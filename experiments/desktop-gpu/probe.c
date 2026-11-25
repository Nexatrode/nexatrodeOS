/* SPDX-License-Identifier: MIT
 * Disposable capability probe. No disk, network, app surfaces or private keys.
 * Validates fenced VirGL clears and retained-texture blits against CPU pixels.
 * Does not establish host GPU identity, desktop performance or presentation. */
#include <microkit.h>
#include <stdint.h>
#include <stddef.h>
#ifdef GPU_PROBE_DESKTOP
#define WIDTH 2560u
#define HEIGHT 1600u
#else
#define WIDTH 64u
#define HEIGHT 64u
#endif
uintptr_t mmio, dma, dma_paddr;
static uintptr_t gpu;
static uint16_t submitted;
struct descriptor { uint64_t address; uint32_t length; uint16_t flags,next; };
struct header { uint32_t type,flags; uint64_t fence; uint32_t context; uint8_t ring,padding[3]; };
_Static_assert(sizeof(struct header)==24,"VirtIO GPU header");
static void clear(void *p,size_t n) { volatile uint8_t *b=p;while(n--)*b++=0; }
static uint64_t ticks(void) { uint64_t n;asm volatile("mrs %0,cntpct_el0":"=r"(n));return n; }
static uint64_t frequency(void) { uint64_t n;asm volatile("mrs %0,cntfrq_el0":"=r"(n));return n; }
static void number(uint64_t n) { char b[24];unsigned i=0;do{b[i++]=(char)('0'+n%10);n/=10;}while(n);while(i)microkit_dbg_putc(b[--i]); }
static void field(const char *s,uint64_t n) {microkit_dbg_puts(s);number(n);microkit_dbg_puts("\n");}
static uint32_t read_reg(unsigned o) {return *(volatile uint32_t*)(gpu+o);}
static void write_reg(unsigned o,uint32_t n) {*(volatile uint32_t*)(gpu+o)=n;asm volatile("dmb sy":::"memory");}
static void address(unsigned o,uint64_t a) {write_reg(o,(uint32_t)a);write_reg(o+4,a>>32);}
static struct header *request(void) {clear((void*)(dma+0x1000),4096);return (void*)(dma+0x1000);}
static struct header *command(uint32_t type,uint32_t context,unsigned bytes) {
    struct header *q=(void*)(dma+0x1000);q->type=type;q->flags=1;q->fence=submitted+1;q->context=context;
    clear((void*)(dma+0x2000),65536);
    volatile struct descriptor *d=(void*)dma;
    d[0]=(struct descriptor){dma_paddr+0x1000,bytes,1,1};
    d[1]=(struct descriptor){dma_paddr+0x2000,65536,2,0};
    volatile uint16_t *avail=(void*)(dma+0x100),*used=(void*)(dma+0x200);
    avail[2+submitted%8]=0;asm volatile("dmb sy":::"memory");avail[1]=++submitted;
    write_reg(0x50,0);uint64_t end=ticks()+frequency()*3;
    while(used[1]!=submitted)if(ticks()>end){microkit_dbg_puts("GPU_PROBE|FAIL|queue timeout\n");return NULL;}
    asm volatile("dmb sy":::"memory");
    volatile uint32_t *entry=(void*)(dma+0x204+8*((submitted-1)%8));
    struct header *r=(void*)(dma+0x2000);
    if(entry[0]!=0 || entry[1]<sizeof(*r) || entry[1]>65536 || !(r->flags&1) || r->fence!=submitted){microkit_dbg_puts("GPU_PROBE|FAIL|completion validation\n");return NULL;}
    field("GPU_PROBE|response=",r->type);return r;
}
static int texture_check(void) {
    struct header *r;request();uint32_t *v=(void*)(dma+0x1018);
    v[0]=8;v[1]=2;v[2]=1;v[3]=10;v[4]=64;v[5]=64;v[6]=1;v[7]=1;
    r=command(0x204,0,72);if(!r||r->type!=0x1100)return 0;
    volatile uint8_t *src=(void*)(dma+0x2000000);
    for(unsigned y=0;y<64;++y)for(unsigned x=0;x<64;++x){unsigned i=(y*64+x)*4,b=((x/8)^(y/8))&1;
        src[i]=b?255:0;src[i+1]=b?0:255;src[i+2]=0;src[i+3]=255;}
    request();v=(void*)(dma+0x1018);v[0]=8;v[1]=1;*(uint64_t*)(dma+0x1020)=dma_paddr+0x2000000;v[4]=16384;
    r=command(0x106,0,48);if(!r||r->type!=0x1100)return 0;
    request();*(uint32_t*)(dma+0x1018)=8;r=command(0x202,1,32);if(!r||r->type!=0x1100)return 0;
    request();v=(void*)(dma+0x1018);v[3]=64;v[4]=64;v[5]=1;v[8]=8;v[10]=256;v[11]=16384;
    r=command(0x205,1,72);if(!r||r->type!=0x1100)return 0;
    uint64_t start=ticks();
    for(unsigned frame=0;frame<16;++frame){
        request();v=(void*)(dma+0x1020);
        const uint32_t clear[]={7|(8<<16),4,0x3f800000,0,0,0x3f800000,0,0,0};
        for(unsigned i=0;i<9;++i)v[i]=clear[i];
        for(unsigned q=0;q<3;++q){uint32_t *b=v+9+22*q;
            b[0]=16|(21<<16);b[1]=15;b[4]=7;b[6]=1;b[7]=q==2?24:q==1?40:frame*2;b[8]=q==2?24:q==1?8:32;
            b[10]=16;b[11]=16;b[12]=1;b[13]=8;b[15]=1;b[19]=64;b[20]=64;b[21]=1;
        }
        *(uint32_t*)(dma+0x1018)=75*4;r=command(0x207,1,32+75*4);if(!r||r->type!=0x1100)return 0;
        request();v=(void*)(dma+0x1018);v[3]=WIDTH;v[4]=HEIGHT;v[5]=1;v[8]=7;v[10]=WIDTH*4;v[11]=WIDTH*HEIGHT*4;
        r=command(0x206,1,72);if(!r||r->type!=0x1100)return 0;
        volatile uint8_t *dst=(void*)(dma+0x20000);
        for(unsigned y=0;y<64;++y)for(unsigned x=0;x<64;++x){unsigned i=(y*WIDTH+x)*4;
            int tx=-1,ty=-1;
            if(x>=frame*2&&x<frame*2+16&&y>=32&&y<48){tx=x-frame*2;ty=y-32;}
            if(x>=40&&x<56&&y>=8&&y<24){tx=x-40;ty=y-8;}
            if(x>=24&&x<40&&y>=24&&y<40){tx=x-24;ty=y-24;}
            unsigned blue=tx>=0?(((tx/2)^(ty/2))&1)*255:0;
            unsigned green=tx>=0?255-blue:0,red=tx>=0?0:255;
            if(dst[i]!=blue||dst[i+1]!=green||dst[i+2]!=red||dst[i+3]!=255){
                field("GPU_PROBE|FAIL|texture_frame=",frame);field("GPU_PROBE|FAIL|texture_pixel=",y*64+x);return 0;}
        }
    }
    field("GPU_PROBE|texture_test_us=",(ticks()-start)*1000000/frequency());
    microkit_dbg_puts("GPU_PROBE|TEXTURES|PASS|frames=16|quads=3|source_uploads=1|readback_verified\n");return 1;
}
static int render_check(void) {
    struct header *r;request();uint32_t *v=(void*)(dma+0x1018);
    v[0]=7;v[1]=2;v[2]=1;v[3]=258;v[4]=WIDTH;v[5]=HEIGHT;v[6]=1;v[7]=1;
    r=command(0x204,0,72);if(!r||r->type!=0x1100)return 0;
    request();v=(void*)(dma+0x1018);v[0]=7;v[1]=1;
    *(uint64_t*)(dma+0x1020)=dma_paddr+0x20000;v[4]=WIDTH*HEIGHT*4;
    r=command(0x106,0,48);if(!r||r->type!=0x1100)return 0;
    request();*(uint32_t*)(dma+0x1018)=7;
    r=command(0x202,1,32);if(!r||r->type!=0x1100)return 0;
    request();v=(void*)(dma+0x1020);
    /* VirGL: create render surface, bind framebuffer, clear opaque red.
     * Integer IEEE754 encodings avoid floating-point state in this driver. */
    const uint32_t stream[]={1|(8<<8)|(5<<16),1,7,1,0,0,
                            5|(3<<16),1,0,1,
                            7|(8<<16),4,0x3f800000,0,0,0x3f800000,0,0,0};
    for(unsigned i=0;i<sizeof(stream)/4;++i)v[i]=stream[i];
    *(uint32_t*)(dma+0x1018)=sizeof(stream);
#ifndef GPU_PROBE_SKIP_CLEAR
    r=command(0x207,1,32+sizeof(stream));if(!r||r->type!=0x1100)return 0;
#endif
    request();v=(void*)(dma+0x1018);v[3]=WIDTH;v[4]=HEIGHT;v[5]=1;
    v[8]=7;v[10]=WIDTH*4;v[11]=WIDTH*HEIGHT*4;
    r=command(0x206,1,72);if(!r||r->type!=0x1100)return 0;
    volatile uint8_t *pixels=(void*)(dma+0x20000);
    for(unsigned i=0;i<WIDTH*HEIGHT;++i)if(((volatile uint32_t*)pixels)[i]!=0xffff0000u){
        field("GPU_PROBE|FAIL|pixel_index=",i);return 0;
    }
    microkit_dbg_puts("GPU_PROBE|RENDER_READBACK|PASS|BGRA=0000ffff\n");
    return 1;
}
void init(void) {
    microkit_dbg_puts("GPU_PROBE|BEGIN|capability-only\n");
    for(unsigned o=0;o<0x4000;o+=0x200){uintptr_t p=mmio+o;
        if(*(volatile uint32_t*)p==0x74726976 && *(volatile uint32_t*)(p+8)==16){if(gpu){microkit_dbg_puts("GPU_PROBE|FAIL|multiple GPUs\n");return;}gpu=p;}}
    if(!gpu){microkit_dbg_puts("GPU_PROBE|FAIL|no GPU\n");return;}
    field("GPU_PROBE|mmio_version=",read_reg(4));if(read_reg(4)!=2){microkit_dbg_puts("GPU_PROBE|FAIL|requires modern MMIO\n");return;}
    write_reg(0x70,0);write_reg(0x70,1);write_reg(0x70,3);
    write_reg(0x14,0);uint32_t low=read_reg(0x10);write_reg(0x14,1);uint32_t high=read_reg(0x10);
    field("GPU_PROBE|features_low=",low);field("GPU_PROBE|features_high=",high);
    if(!(high&1)){microkit_dbg_puts("GPU_PROBE|FAIL|VERSION_1 absent\n");return;}
    write_reg(0x24,0);write_reg(0x20,low&1);write_reg(0x24,1);write_reg(0x20,1);write_reg(0x70,11);
    if(!(read_reg(0x70)&8)){microkit_dbg_puts("GPU_PROBE|FAIL|feature rejection\n");return;}
    clear((void*)dma,0x12000);write_reg(0x30,0);
    if(read_reg(0x34)<8){microkit_dbg_puts("GPU_PROBE|FAIL|queue too small\n");return;}
    write_reg(0x38,8);address(0x80,dma_paddr);address(0x90,dma_paddr+0x100);address(0xa0,dma_paddr+0x200);write_reg(0x44,1);write_reg(0x70,15);
    uint32_t count=read_reg(0x10c);field("GPU_PROBE|capsets=",count);
    if(count>16){microkit_dbg_puts("GPU_PROBE|FAIL|capset bound\n");return;}
    for(uint32_t i=0;i<count;++i){request();*(uint32_t*)(dma+0x1018)=i;
        struct header *r=command(0x108,0,32);if(!r||r->type!=0x1102)return;
        uint32_t *v=(void*)(r+1),id=v[0],version=v[1],size=v[2];
        field("GPU_PROBE|capset_id=",id);field("GPU_PROBE|capset_version=",version);field("GPU_PROBE|capset_bytes=",size);
        if(!size||size>65536-sizeof(*r)){microkit_dbg_puts("GPU_PROBE|FAIL|capset size\n");return;}
        request();uint32_t *a=(void*)(dma+0x1018);a[0]=id;a[1]=version;
        r=command(0x109,0,32);if(!r||r->type!=0x1103)return;
    }
    if(low&1){request();struct header *q=(void*)(dma+0x1000);q->context=1;
        uint32_t *v=(void*)(q+1);v[0]=9;char *name=(void*)(v+2);const char *s="nexatrode";for(unsigned i=0;i<9;++i)name[i]=s[i];
        struct header *r=command(0x200,1,96);if(!r||r->type!=0x1100)return;
        if(!render_check()||!texture_check())return;
        request();v=(void*)(dma+0x1018);v[2]=WIDTH;v[3]=HEIGHT;v[5]=7;
        r=command(0x103,0,48);if(!r||r->type!=0x1100)return;
        request();v=(void*)(dma+0x1018);v[2]=WIDTH;v[3]=HEIGHT;v[4]=7;
        r=command(0x104,0,48);if(!r||r->type!=0x1100)return;
        field("GPU_PROBE|scanout_width=",WIDTH);field("GPU_PROBE|scanout_height=",HEIGHT);
        /* Keep the context alive until device reset so scanout can consume it. */
        microkit_dbg_puts("GPU_PROBE|VIRGL_CONTEXT|PASS\n");
    }
    microkit_dbg_puts("GPU_PROBE|DONE|capability_and_optional_readback\n");
}
void notified(microkit_channel c) {(void)c;}
