/* SPDX-License-Identifier: MIT
 * Experimental trusted VirGL compositor. App surfaces are read-only inputs;
 * only this PD owns GPU MMIO and DMA. Slint remains a software renderer.
 */
#include <microkit.h>
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include <stdatomic.h>
#include "../desktop-shell/protocol.h"
#include "../../tests/input/slint_frame.h"
void *memcpy(void *to,const void *from,size_t n) {uint8_t *d=to;const uint8_t *s=from;while(n--)*d++=*s++;return to;}
void *memset(void *to,int c,size_t n) {uint8_t *d=to;while(n--)*d++=c;return to;}
uintptr_t mmio, dma, dma_paddr, shell_surface, shell_state, ui_pointer;
uintptr_t surface_a, surface_b, surface_c, output_frame, shell_decorations, gpu_observation;
#include "transport.h"
#include "pacing.h"
#ifdef DESKTOP_DYNAMIC
#include "display_protocol.h"
uintptr_t desktop_display_mode,display_ack;
#define MAX_W DISPLAY_MAX_W
#define MAX_H DISPLAY_MAX_H
static uint32_t mode_revision,mode_serial;
#else
#define MAX_W (1280u*DESKTOP_SCALE)
#define MAX_H (800u*DESKTOP_SCALE)
#endif
#define W SHELL_PIXEL_WIDTH
#define H SHELL_PIXEL_HEIGHT
#define AW APP_PIXEL_WIDTH
#define AH APP_PIXEL_HEIGHT
#define SHELL_BYTES (MAX_W*MAX_H*3u)
#define APP_BYTES (AW*APP_MAX_PIXEL_HEIGHT*3u)
#define DW (562u*DESKTOP_SCALE)
#define DH ((APP_HEIGHT+44u)*DESKTOP_SCALE)
#define MAX_DH ((APP_MAX_HEIGHT+44u)*DESKTOP_SCALE)
#define DECOR_BYTES (DW*MAX_DH*3u)
static uint8_t shell_cache[SHELL_BYTES], app_cache[3][APP_BYTES], scratch[SHELL_BYTES];
static uint8_t decor_cache[3][DECOR_BYTES];
static uint8_t *cache[7]={shell_cache,app_cache[0],app_cache[1],app_cache[2],decor_cache[0],decor_cache[1],decor_cache[2]};
static uint32_t revisions[7], pointer_revision, scene_revision;
static struct shell_state scene;
#ifdef DESKTOP_DYNAMIC
static uint32_t offsets[9]={0x200000,0x2200000,0x4200000,0x6200000,0x6800000,0x6e00000,0x7400000,0x7a00000,0x8000000};
_Static_assert(0x8000000+DW*MAX_DH*4<=0x9000000,"bounded resize DMA textures");
#else
static uint32_t offsets[9]={0x20000,0x1020000,0x2020000,0x3020000,0x34a0000,0x3920000,0x3e00000,0x4400000,0x4a00000};
_Static_assert(0x4a00000+DW*MAX_DH*4<=0x6000000,"bounded DMA textures");
#endif
static const unsigned cursor_width[11]={1,2,3,4,5,6,7,4,3,2,1};
#define CURSOR_UNIT (4u*DESKTOP_SCALE)
static const unsigned round_inset[14]={11,8,6,5,4,3,2,2,1,1,1,0,0,0};
static uint32_t stream[4000], words, target, frames, uploads;
/* Kept as an ELF symbol for the disposable QMP completion benchmark. This
 * advances only after both scanout and flush commands complete successfully. */
static volatile uint32_t presented_scene_revision;
static bool ready, failed;
enum frame_phase { FRAME_IDLE, FRAME_PREPARING, FRAME_UPLOADS, FRAME_PRESENT };
static enum frame_phase phase;
static struct frame_pacer pacer;
static struct {
    uint64_t begin,wait_begin,snapshot_end,compose_end,handled_ticks,start_gap,pending_age,last_poll,max_poll_gap;
    uint32_t old_uploads,commands_begin,snapshot_index,draw_fence,present_fence,poll_calls;
    bool dirty;
#ifdef DESKTOP_GPU_VERIFY
    int pointer_x,pointer_y;bool pointer_visible;
#endif
} work;
static uint64_t handler_begin,previous_start,watchdog_next,watchdog_deadline,timer_at;
static uint32_t gpu_irqs,timer_wakes,watchdog_recoveries,irq_batches,timer_batches,max_pending;
static void drive(unsigned source);
static void scheduler_observe(void) {
    volatile uint32_t *s=(void*)gpu_observation;uint32_t r=s[0];s[0]=r+1;__atomic_thread_fence(__ATOMIC_SEQ_CST);
    s[15]=0x50414331;s[16]=gpu_irqs;s[17]=timer_wakes;s[18]=pacer.coalesced;s[19]=pacer.deferrals;
    s[20]=watchdog_recoveries;s[21]=phase;s[22]=max_pending;s[23]=synchronous_command_count;
    s[24]=irq_batches;s[25]=timer_batches;s[26]=20;
    __atomic_thread_fence(__ATOMIC_RELEASE);s[0]=r+2;
}
static void arm_timer(uint64_t deadline) {
    if(!deadline||timer_at==deadline)return;
    uint64_t now=ticks(),delay=deadline>now?deadline-now:1;
    timer_at=deadline;microkit_mr_set(0,(delay*1000000000+frequency()-1)/frequency());
    microkit_ppcall(2,microkit_msginfo_new(1,1));
}
static void wait_for_batch(enum frame_phase next) {
    phase=next;uint64_t now=ticks();watchdog_next=now+frequency()/20;watchdog_deadline=now+frequency()*3;
    if(pending_commands>max_pending)max_pending=pending_commands;
    work.last_poll=now;work.handled_ticks+=now-handler_begin;kick_batch();
}
#ifdef DESKTOP_DYNAMIC
static bool targets_dirty;
#endif
static bool ok(struct header *r) { if(r && r->type==0x1100)return true; failed=true;microkit_dbg_puts("DESKTOP_GPU|FAIL|response\n");field("DESKTOP_GPU|failed_command=",((struct header*)(dma+GPU_REQUEST))->type);field("DESKTOP_GPU|failed_response=",r?r->type:0);return false; }
static bool submit(void) {
    if(!words)return true;
    request();uint32_t *v=(void*)(dma+GPU_REQUEST+32);
    for(unsigned i=0;i<words;++i)v[i]=stream[i];
    *(uint32_t*)(dma+GPU_REQUEST+24)=words*4;
    bool success=defer_command(0x207,1,32+words*4);words=0;if(!success)failed=true;return success;
}
static bool blit(unsigned source,int sx,int sy,int dx,int dy,unsigned width,unsigned height) {
    if(!width||!height)return true;
    if(dx<0||dy<0||dx+(uint64_t)width>W||dy+(uint64_t)height>H){failed=true;microkit_dbg_puts("DESKTOP_GPU|FAIL|blit bounds\n");return false;}
    if(words+22>4000 && !submit())return false;
    uint32_t *b=stream+words;for(unsigned i=0;i<22;++i)b[i]=0;
    b[0]=16|(21<<16);b[1]=15;b[4]=target;b[6]=1;b[7]=dx;b[8]=dy;b[10]=width;b[11]=height;b[12]=1;
    b[13]=source;b[15]=1;b[16]=sx;b[17]=sy;b[19]=width;b[20]=height;b[21]=1;words+=22;return true;
}
static bool resource(unsigned id,unsigned width,unsigned height,unsigned offset,bool display) {
    request();uint32_t *v=(void*)(dma+GPU_REQUEST+24);v[0]=id;v[1]=2;v[2]=1;v[3]=display?258:10;v[4]=width;v[5]=height;v[6]=1;v[7]=1;
    if(!ok(command(0x204,0,72)))return false;
    request();v=(void*)(dma+GPU_REQUEST+24);v[0]=id;v[1]=1;*(uint64_t*)(dma+GPU_REQUEST+32)=dma_paddr+offset;v[4]=width*height*4;
    if(!ok(command(0x106,0,48)))return false;
    request();*(uint32_t*)(dma+GPU_REQUEST+24)=id;return ok(command(0x202,1,32));
}
#ifdef DESKTOP_DYNAMIC
/* SPICE uses the scanout texture's backing dimensions for tablet coordinates.
 * Keep these two textures at the actual mode, retaining bounded maximum DMA
 * capacity. Every command completes with a fence before an old resource dies. */
static bool resize_targets(void) {
    request();if(!ok(command(0x103,0,48)))return false; /* disable old scanout */
    for(unsigned id=7;id<=8;++id){
        request();*(uint32_t*)(dma+GPU_REQUEST+24)=id;if(!ok(command(0x203,1,32)))return false;
        request();*(uint32_t*)(dma+GPU_REQUEST+24)=id;if(!ok(command(0x107,0,32)))return false;
        request();*(uint32_t*)(dma+GPU_REQUEST+24)=id;if(!ok(command(0x102,0,32)))return false;
    }
    if(!resource(7,W,H,offsets[0],true)||!resource(8,W,H,offsets[1],true))return false;
    targets_dirty=false;return true;
}
#endif
static bool upload(unsigned i,unsigned width,unsigned height) {
    volatile uint32_t *dest=(void*)(dma+offsets[i+2]);const uint8_t *src=cache[i];
    unsigned stride=i==0?MAX_W:width,resource_height=i==0?MAX_H:i<=3?APP_MAX_PIXEL_HEIGHT:MAX_DH;
    for(unsigned y=0;y<height;++y)for(unsigned x=0;x<width;++x){unsigned n=y*width+x;dest[y*stride+x]=0xff000000u|((uint32_t)src[n*3]<<16)|((uint32_t)src[n*3+1]<<8)|src[n*3+2];}
    request();uint32_t *v=(void*)(dma+GPU_REQUEST+24);v[3]=width;v[4]=height;v[5]=1;v[8]=10+i;v[10]=stride*4;v[11]=stride*resource_height*4;
    ++uploads;bool success=defer_command(0x205,1,72);if(!success){failed=true;microkit_dbg_puts("DESKTOP_GPU|FAIL|upload queue\n");}return success;
}
static bool snapshot(unsigned i,uintptr_t source,unsigned width,unsigned height) {
    /* Shell and app header sizes differ. Copy into a cached staging buffer,
     * then recheck the producer generation before replacing a retained image. */
    const slint_frame_header_t *h=(void*)source;
    uint32_t rev=atomic_load_explicit(&h->revision,memory_order_acquire);
    if(!rev||(rev&1)||rev==revisions[i])return false;
    const uint32_t *fields=(void*)source;
    if(fields[1]!=width||fields[2]!=height||fields[3]!=width*3)return false;
    unsigned header=(i>=1&&i<=3)?sizeof(slint_frame_header_t):16;
    if(i>=1&&i<=3 && fields[4]!=SLINT_FRAME_FORMAT_RGB8)return false;
    const volatile uint8_t *p=(void*)(source+header);
    for(unsigned n=0;n<width*height*3;++n)scratch[n]=p[n];
    atomic_thread_fence(memory_order_acquire);
    if(atomic_load_explicit(&h->revision,memory_order_acquire)!=rev)return false;
    for(unsigned n=0;n<width*height*3;++n)cache[i][n]=scratch[n];
    if(!upload(i,width,height))return false;
    revisions[i]=rev;return true;
}
static bool read_scene(void) {
    const struct shell_state *p=(void*)shell_state;
    uint32_t rev=__atomic_load_n(&p->revision,__ATOMIC_ACQUIRE);
    if(!rev||(rev&1)||rev==scene_revision)return false;
    struct shell_state next=*p;__atomic_thread_fence(__ATOMIC_ACQUIRE);
    if(__atomic_load_n(&p->revision,__ATOMIC_ACQUIRE)!=rev)return false;
    unsigned order[DESKTOP_APP_COUNT];shell_order(&next,order);unsigned seen=0;
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i){
        if(order[i]<1||order[i]>DESKTOP_APP_COUNT||(seen&(1u<<order[i])))return false;seen|=1u<<order[i];
        struct shell_window *w=&next.windows[i];
        if(w->visible&&(w->x<1||w->y<43||w->x+(int)APP_WIDTH+1>(int)SHELL_WIDTH||w->y+(int)APP_HEIGHT+1>(int)SHELL_HEIGHT))return false;
    }
    scene=next;scene_revision=rev;return true;
}
#ifdef DESKTOP_GPU_VERIFY
static uint32_t reference(unsigned x,unsigned y,int px,int py,bool pointer_visible) {
    const uint8_t *p=cache[0]+(y*W+x)*3;
    unsigned order[DESKTOP_APP_COUNT];shell_order(&scene,order);
    static const unsigned corner[10]={0,0,0,1,1,2,3,4,6,9};
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i){unsigned id=order[i];struct shell_window *w=&scene.windows[id-1];
        if(!w->visible||!revisions[id+3])continue;
        int left=w->x*DESKTOP_SCALE,top=w->y*DESKTOP_SCALE;
        int dx=(int)x-left+DESKTOP_SCALE,dy=(int)y-top+43*DESKTOP_SCALE;
        if(dx>=0&&dy>=0&&dx<(int)DW&&dy<(int)DH){
            unsigned row=dy/DESKTOP_SCALE,edge=row<14?row:row>=APP_HEIGHT+44-14?APP_HEIGHT+43-row:14;
            unsigned inset=edge<14?round_inset[edge]*DESKTOP_SCALE:0;
            if((unsigned)dx>=inset&&(unsigned)dx<DW-inset)p=cache[id+3]+((unsigned)dy*DW+dx)*3;
        }
        if(revisions[id]&&(int)x>=left&&(int)x<left+(int)AW&&(int)y>=top&&(int)y<top+(int)AH){
            unsigned row=(y-top)/DESKTOP_SCALE,inset=row>=APP_HEIGHT-10?corner[row-(APP_HEIGHT-10)]*DESKTOP_SCALE:0;
            if(x-left>=inset&&x-left<AW-inset)p=cache[id]+((y-top)*AW+x-left)*3;
        }
    }
    if(pointer_visible&&(int)y>=py&&(int)y<py+11*(int)CURSOR_UNIT&&(int)x>=px&&(int)x<px+(int)(cursor_width[(y-py)/CURSOR_UNIT]*CURSOR_UNIT))return 0xff40f4ffu;
    return 0xff000000u|((uint32_t)p[0]<<16)|((uint32_t)p[1]<<8)|p[2];
}
static bool readback(int px,int py,bool pointer_visible) {
    request();uint32_t *v=(void*)(dma+GPU_REQUEST+24);v[3]=W;v[4]=H;v[5]=1;v[8]=target;v[10]=W*4;v[11]=W*H*4;
    if(!ok(command(0x206,1,72)))return false;
    volatile uint32_t *pixels=(void*)(dma+offsets[target-7]);
    for(unsigned y=0;y<H;y+=7)for(unsigned x=0;x<W;x+=7){
        if(pixels[y*W+x]!=reference(x,y,px,py,pointer_visible)){
            microkit_dbg_puts("DESKTOP_GPU|FAIL|CPU-reference mismatch\n");field("DESKTOP_GPU|x=",x);field("DESKTOP_GPU|y=",y);failed=true;return false;
        }
    }
    slint_frame_header_t *out=(void*)output_frame;uint32_t before=atomic_load(&out->revision);
    atomic_store(&out->revision,before+1);atomic_thread_fence(memory_order_seq_cst);
    uint8_t *rgb=(void*)(out+1);
    for(unsigned y=0;y<H;++y)for(unsigned x=0;x<W;++x){unsigned n=y*W+x;uint32_t c=pixels[y*W+x];rgb[3*n]=c>>16;rgb[3*n+1]=c>>8;rgb[3*n+2]=c;}
    out->width=W;out->height=H;out->stride_bytes=W*3;out->format=SLINT_FRAME_FORMAT_RGB8;
    atomic_store_explicit(&out->revision,before+2,memory_order_release);return true;
}
#endif
#ifdef DESKTOP_DYNAMIC
static bool read_mode(void) {
    const struct display_mode *m=(void*)desktop_display_mode;uint32_t revision=__atomic_load_n(&m->revision,__ATOMIC_ACQUIRE);struct display_mode copy=*m;
    __atomic_thread_fence(__ATOMIC_ACQUIRE);
    if(!revision||(revision&1)||revision==mode_revision||revision!=__atomic_load_n(&m->revision,__ATOMIC_ACQUIRE)||copy.scale!=2||copy.width<DISPLAY_MIN_W||copy.height<DISPLAY_MIN_H||copy.width>MAX_W||copy.height>MAX_H||(copy.width&1)||(copy.height&1)||copy.app_height<360||copy.app_height>520)return false;
    if(W!=copy.width||H!=copy.height){revisions[0]=0;targets_dirty=true;}
    if(APP_HEIGHT!=copy.app_height)for(unsigned i=1;i<7;++i)revisions[i]=0;
    desktop_width=copy.width/2;desktop_height=copy.height/2;desktop_app_height=copy.app_height;
    mode_revision=revision;mode_serial=copy.serial;return true;
}
#endif
static void prepare_frame(void) {
    while(work.snapshot_index<7){
        if(pending_commands==GPU_BATCH_SLOTS){wait_for_batch(FRAME_UPLOADS);return;}
        unsigned i=work.snapshot_index++;
        if(i==0)work.dirty|=snapshot(0,shell_surface,W,H);
        else if(i<=3){uintptr_t source=i==1?surface_a:i==2?surface_b:surface_c;work.dirty|=snapshot(i,source,AW,AH);}
        else work.dirty|=snapshot(i,shell_decorations+(i-4)*0x400000,DW,DH);
        if(failed)return;
    }
    if(pending_commands){wait_for_batch(FRAME_UPLOADS);return;}
    const volatile uint32_t *pointer=(void*)ui_pointer;
    uint32_t pr=pointer[0];int px=pointer[1],py=pointer[2];bool visible=pointer[3];
    __atomic_thread_fence(__ATOMIC_ACQUIRE);
    bool pointer_ok=!(pr&1)&&pr==pointer[0];
    if(pointer_ok&&pointer_revision!=pr){pointer_revision=pr;work.dirty=true;}
    if(!work.dirty||!revisions[0]||!scene_revision||failed){phase=FRAME_IDLE;return;}
    work.snapshot_end=ticks();
    target=7+(frames&1);words=0;
    if(!blit(10,0,0,0,0,W,H))return;
    unsigned order[DESKTOP_APP_COUNT];shell_order(&scene,order);
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i){unsigned id=order[i];struct shell_window *w=&scene.windows[id-1];
        if(!w->visible||!revisions[id+3])continue;
        int x=w->x*DESKTOP_SCALE,y=w->y*DESKTOP_SCALE;
        /* Move a complete retained decoration and content as one scene. */
        int dx=x-DESKTOP_SCALE,dy=y-43*DESKTOP_SCALE;
        /* Merge adjacent equal-inset rows. The pixel mask is unchanged, but
         * three visible windows need 84 blits including cursor, rather than
         * 132 separately dispatched strips. No new image upload is needed. */
        unsigned edge=0;while(edge<14&&round_inset[edge])++edge;
        if(!blit(13+id,0,edge*DESKTOP_SCALE,dx,dy+edge*DESKTOP_SCALE,DW,DH-2*edge*DESKTOP_SCALE))return;
        for(unsigned r=0;r<edge;){
            unsigned n=1;while(r+n<edge&&round_inset[r+n]==round_inset[r])++n;
            unsigned inset=round_inset[r]*DESKTOP_SCALE;
            if(!blit(13+id,inset,r*DESKTOP_SCALE,dx+inset,dy+r*DESKTOP_SCALE,DW-2*inset,n*DESKTOP_SCALE))return;
            unsigned row=DH-(r+n)*DESKTOP_SCALE;
            if(!blit(13+id,inset,row,dx+inset,dy+row,DW-2*inset,n*DESKTOP_SCALE))return;
            r+=n;
        }
        if(!revisions[id])continue;
        static const unsigned corner[10]={0,0,0,1,1,2,3,4,6,9};
        unsigned first=0;while(first<10&&!corner[first])++first;
        if(!blit(10+id,0,0,x,y,AW,AH-(10-first)*DESKTOP_SCALE))return;
        for(unsigned r=first;r<10;){
            unsigned n=1;while(r+n<10&&corner[r+n]==corner[r])++n;
            unsigned inset=corner[r]*DESKTOP_SCALE,row=AH-20+r*DESKTOP_SCALE;
            if(!blit(10+id,inset,row,x+inset,y+row,AW-2*inset,n*DESKTOP_SCALE))return;
            r+=n;
        }
    }
    /* Cursor uses opaque one-pixel spans: no alpha shader is needed. The
     * source is a retained cyan patch in the reserved scratch DMA page. */
    if(pointer_ok&&visible&&px>=0&&py>=0&&px<(int)W&&py<(int)H){
        for(unsigned row=0;row<11;++row){unsigned y=py+row*CURSOR_UNIT;if(y>=H)break;
            unsigned width=cursor_width[row]*CURSOR_UNIT,height=CURSOR_UNIT;
            if(px+(int)width>(int)W)width=W-px;if(y+height>H)height=H-y;
            if(!blit(20,0,0,px,y,width,height))return;}
    }
    if(!submit())return;
    work.draw_fence=(uint32_t)next_fence;
#ifdef DESKTOP_GPU_VERIFY
    work.pointer_x=px;work.pointer_y=py;work.pointer_visible=pointer_ok&&visible;
#endif
    work.compose_end=ticks();
    request();uint32_t *v=(void*)(dma+GPU_REQUEST+24);v[2]=W;v[3]=H;v[5]=target;if(!defer_command(0x103,0,48)){failed=true;return;}
    request();v=(void*)(dma+GPU_REQUEST+24);v[2]=W;v[3]=H;v[4]=target;if(!defer_command(0x104,0,48)){failed=true;return;}
    work.present_fence=(uint32_t)next_fence;
    wait_for_batch(FRAME_PRESENT);
}
static void complete_frame(unsigned source) {
#ifdef DESKTOP_GPU_VERIFY
    /* Validate the same final-fence submission as the normal image. Reading
     * before presentation would accidentally add an extra draw fence. */
    if(!readback(work.pointer_x,work.pointer_y,work.pointer_visible))return;
#endif
    uint64_t begin=work.begin,wait_begin=work.wait_begin,snapshot_end=work.snapshot_end,compose_end=work.compose_end;
    unsigned old_uploads=work.old_uploads,commands_begin=work.commands_begin;
    presented_scene_revision=scene_revision;
    __atomic_add_fetch(&frames,1,__ATOMIC_RELEASE);
    volatile uint32_t *stats=(void*)gpu_observation;uint32_t generation=stats[0];stats[0]=generation+1;__atomic_thread_fence(__ATOMIC_SEQ_CST);
    stats[1]=presented_scene_revision;stats[2]=frames;stats[3]=uploads;stats[4]=(ticks()-begin)*1000000/frequency();
    stats[5]=revisions[0];stats[6]=revisions[4];stats[7]=revisions[5];stats[8]=revisions[6];
    stats[9]=W;stats[10]=H;stats[12]=APP_HEIGHT;
    /* Bounded timing-only history. No app text, pixels or key events. The
     * existing observation seqlock also protects each 80-byte ring record. */
    stats[13]=0x4d4f5431;stats[14]=48;
    _Static_assert((32+48*20)*sizeof(uint32_t)<=4096,"bounded timing history");
    volatile uint32_t *sample=stats+32+((frames-1)%48)*20;
    uint64_t end=ticks(),hz=frequency();
    sample[0]=frames;sample[1]=scene_revision;sample[2]=(uint32_t)((begin/hz)*1000000+(begin%hz)*1000000/hz);
    sample[3]=(end-begin)*1000000/hz;sample[4]=(snapshot_end-begin)*1000000/hz;
    sample[5]=(compose_end-snapshot_end)*1000000/hz;sample[6]=(end-compose_end)*1000000/hz;
    sample[7]=command_count-commands_begin;sample[8]=(command_wait_ticks-wait_begin)*1000000/hz;
    sample[9]=uploads-old_uploads;sample[10]=W;sample[11]=H;
    sample[12]=(work.handled_ticks+ticks()-handler_begin)*1000000/hz;
    sample[13]=work.start_gap*1000000/hz;sample[14]=work.pending_age*1000000/hz;sample[15]=source;
    sample[16]=work.draw_fence;sample[17]=work.present_fence;
    sample[18]=work.max_poll_gap*1000000/hz;sample[19]=work.poll_calls;
#ifdef DESKTOP_DYNAMIC
    stats[11]=mode_serial;
    struct display_ack *ack=(void*)display_ack;uint32_t ar=__atomic_load_n(&ack->revision,__ATOMIC_RELAXED);
    __atomic_store_n(&ack->revision,ar+1,__ATOMIC_RELEASE);__atomic_thread_fence(__ATOMIC_SEQ_CST);
    ack->serial=mode_serial;ack->width=W;ack->height=H;__atomic_store_n(&ack->revision,ar+2,__ATOMIC_RELEASE);
#endif
    __atomic_thread_fence(__ATOMIC_RELEASE);stats[0]=generation+2;
    phase=FRAME_IDLE;
    if(frames<=5 || frames%60==0){field("DESKTOP_GPU|frame=",frames);field("DESKTOP_GPU|uploads=",uploads-old_uploads);field("DESKTOP_GPU|frame_us=",(ticks()-begin)*1000000/frequency());}
}
static void drive(unsigned source) {
    if(!ready||failed)return;
    uint64_t now=ticks();
    if(phase==FRAME_UPLOADS||phase==FRAME_PRESENT){
        uint64_t gap=now-work.last_poll;work.last_poll=now;if(gap>work.max_poll_gap)work.max_poll_gap=gap;++work.poll_calls;
        int result=poll_batch();
        if(result<0){failed=true;return;}
        if(!result){
            work.handled_ticks+=ticks()-handler_begin;
            if(now>=watchdog_deadline){failed=true;microkit_dbg_puts("DESKTOP_GPU|FAIL|completion deadline\n");return;}
            if(now>=watchdog_next)watchdog_next=now+frequency()/20;
            arm_timer(watchdog_next);return;
        }
        if(source==1)++irq_batches;
        else if(source==2){++timer_batches;if(now>=watchdog_next)++watchdog_recoveries;}
        if(phase==FRAME_UPLOADS){phase=FRAME_PREPARING;prepare_frame();}
        else complete_frame(source);
        if(phase==FRAME_UPLOADS||phase==FRAME_PRESENT){arm_timer(watchdog_next);return;}
        handler_begin=ticks();
    }
    if(phase==FRAME_IDLE&&pacer.pending){
        now=ticks();
        if(!frame_pacer_ready(&pacer,now,false)){++pacer.deferrals;arm_timer(pacer.next_start);return;}
        work=(__typeof__(work)){.begin=now,.wait_begin=command_wait_ticks,.old_uploads=uploads,.commands_begin=command_count,
            .start_gap=previous_start?now-previous_start:0,.pending_age=frame_pacer_take(&pacer,now)};
        previous_start=now;phase=FRAME_PREPARING;
#ifdef DESKTOP_DYNAMIC
        work.dirty|=read_mode();
        if(targets_dirty&&!resize_targets())return;
#endif
        work.dirty|=read_scene();prepare_frame();
        if(phase==FRAME_UPLOADS||phase==FRAME_PRESENT)arm_timer(watchdog_next);
    }
}
void init(void) {
    gpu=mmio;bool found=false;
    for(unsigned o=0;o<0x1000;o+=0x200)if(*(volatile uint32_t*)(mmio+o)==0x74726976&&*(volatile uint32_t*)(mmio+o+8)==16){gpu=mmio+o;found=true;break;}
    if(!found||read_reg(4)!=2){microkit_dbg_puts("DESKTOP_GPU|FAIL|modern GPU absent\n");return;}
    write_reg(0x70,0);write_reg(0x70,1);write_reg(0x70,3);write_reg(0x14,0);uint32_t low=read_reg(0x10);write_reg(0x14,1);uint32_t high=read_reg(0x10);
    if(!(low&1)||!(high&1)){microkit_dbg_puts("DESKTOP_GPU|FAIL|VirGL absent; use CPU image\n");return;}
    write_reg(0x24,0);write_reg(0x20,1);write_reg(0x24,1);write_reg(0x20,1);write_reg(0x70,11);
    if(!(read_reg(0x70)&8))return;
    clear((void*)dma,0x20000);write_reg(0x30,0);if(read_reg(0x34)<8)return;
    write_reg(0x38,8);address(0x80,dma_paddr);address(0x90,dma_paddr+0x100);address(0xa0,dma_paddr+0x200);write_reg(0x44,1);write_reg(0x70,15);
    request();uint32_t *v=(void*)(dma+GPU_REQUEST+24);v[0]=9;char *name=(void*)(v+2);const char *s="nexatrode";for(unsigned i=0;i<9;++i)name[i]=s[i];if(!ok(command(0x200,1,96)))return;
    if(!resource(7,W,H,offsets[0],true)||!resource(8,W,H,offsets[1],true)||!resource(10,MAX_W,MAX_H,offsets[2],false))return;
    for(unsigned i=1;i<=3;++i)if(!resource(10+i,AW,APP_MAX_PIXEL_HEIGHT,offsets[i+2],false))return;
    for(unsigned i=4;i<7;++i)if(!resource(10+i,DW,MAX_DH,offsets[i+2],false))return;
    if(!resource(20,64,88,GPU_CURSOR_BASE,false))return;
    for(unsigned i=0;i<64*88;++i)((volatile uint32_t*)(dma+GPU_CURSOR_BASE))[i]=0xff40f4ffu;
    request();v=(void*)(dma+GPU_REQUEST+24);v[3]=64;v[4]=88;v[5]=1;v[8]=20;v[10]=256;v[11]=64*88*4;if(!ok(command(0x205,1,72)))return;
    pacer.period=(frequency()+59)/60;ready=true;
    microkit_dbg_puts("DESKTOP_GPU|READY|retained-textures|double-buffered|VirGL|paced-irq\n");
    handler_begin=ticks();frame_pacer_request(&pacer,handler_begin);drive(0);scheduler_observe();
}
void notified(microkit_channel channel) {
    handler_begin=ticks();
    if(channel==1){uint32_t status=read_reg(0x60);if(status)write_reg(0x64,status);microkit_irq_ack(1);++gpu_irqs;}
    else if(channel==2){timer_at=0;++timer_wakes;}
    else if(channel==0)frame_pacer_request(&pacer,handler_begin);
    drive(channel);scheduler_observe();
}
