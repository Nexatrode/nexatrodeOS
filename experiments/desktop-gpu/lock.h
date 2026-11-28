/* Full-session cover and retained-surface cleanup. No raw key or auth control. */
#include "../desktop-apps/lock_protocol.h"
uintptr_t desktop_lock_state,lock_display_ack,auth_surface;
static bool lock_active=true,lock_backdrop_ready;
static uint64_t lock_epoch,lock_erased;
static unsigned lock_stage,lock_targets,lock_w,lock_h;
static uint32_t lock_auth_revision,lock_background_revision=2;
static void lock_zero(uintptr_t address,size_t bytes) {
    while((address&7)&&bytes){*(volatile uint8_t*)address++=0;--bytes;}
    volatile uint64_t *p=(void*)address;
    for(size_t n=0;n<bytes/8;++n)p[n]=0;
    volatile uint8_t *tail=(void*)(address+(bytes&~7UL));
    for(size_t n=0;n<(bytes&7);++n)tail[n]=0;
}
static bool lock_begin_frame(void) {
    const struct trio_status *s=(void*)desktop_lock_state;
    uint32_t r=atomic_load_explicit(&s->revision,memory_order_acquire);
    bool locked=s->locked;uint64_t epoch=s->epoch;atomic_thread_fence(memory_order_acquire);
    if(!r||(r&1)||r!=atomic_load(&s->revision)){locked=true;epoch=lock_epoch;}
    bool changed=locked!=lock_active||epoch!=lock_epoch;
    if(changed){
        lock_active=locked;lock_epoch=epoch;lock_stage=lock_targets=0;lock_erased=0;
        lock_auth_revision=0;lock_backdrop_ready=false;revisions[0]=0;scene_revision=0;
        buffer_stamp[0].valid=buffer_stamp[1].valid=false;
        for(unsigned i=1;i<GPU_TEXTURE_COUNT;++i)revisions[i]=0;
    }
    if(lock_w!=W||lock_h!=H){lock_w=W;lock_h=H;lock_targets=0;lock_auth_revision=0;lock_backdrop_ready=false;}
    if(lock_active&&lock_stage==1){
        /* Cover has completed once. Clear readback/presentation backing and
         * scratch before the second covered frame and its final acknowledgement. */
        lock_zero(dma+offsets[0],0x4000000u); /* two bounded 32 MiB targets */
        lock_erased+=0x4000000u;
#ifdef DESKTOP_GPU_VERIFY
        lock_zero(output_frame,0x2000000u);lock_erased+=0x2000000u;
#endif
        lock_zero((uintptr_t)scratch,sizeof(scratch));lock_erased+=sizeof(scratch);
        lock_auth_revision=0;lock_backdrop_ready=false;
    }
    return changed||(lock_active&&lock_epoch&&lock_targets!=3);
}
static bool lock_backdrop(void) {
    const struct lock_surface_header *h=(void*)auth_surface;
    uint32_t rev=atomic_load_explicit(&h->revision,memory_order_acquire);
    bool valid=rev&&!(rev&1)&&h->width==TRIO_AUTH_W&&h->height==TRIO_AUTH_H&&h->stride==TRIO_AUTH_W*3&&
        h->epoch==lock_epoch&&h->locked==1&&!h->reserved;
    if(lock_backdrop_ready&&(!valid||rev==lock_auth_revision))return false;
    clear(scratch,SHELL_BYTES);
    if(valid){
        struct trio_auth_box b=trio_auth_box(W,H);const volatile uint8_t *src=(void*)(h+1);
        for(int y=0;y<b.height;++y)for(int x=0;x<b.width;++x){
            unsigned sy=(uint64_t)y*TRIO_AUTH_H/b.height,sx=(uint64_t)x*TRIO_AUTH_W/b.width;
            size_t to=((y+b.top)*W+x+b.left)*3,from=(sy*TRIO_AUTH_W+sx)*3;
            for(unsigned c=0;c<3;++c)scratch[to+c]=src[from+c];
        }
        atomic_thread_fence(memory_order_acquire);
        if(rev!=atomic_load(&h->revision))return false;
    }
    memcpy(shell_cache,scratch,W*H*3);
    struct gpu_rect full={0,0,W,H};
    if(!upload(0,W,full))return false;
    for(unsigned b=0;b<2;++b)gpu_union(&backdrop_pending[b],full);
    revisions[0]=(lock_background_revision+=2);lock_auth_revision=valid?rev:0;lock_backdrop_ready=true;
    return true;
}
static bool lock_private_surface(unsigned i) {
    size_t bytes=i<=DESKTOP_APP_COUNT?APP_BYTES:DECOR_BYTES;
    lock_zero((uintptr_t)cache[i],bytes);
    volatile uint8_t *p=cache[i];
    for(size_t n=0;n<bytes;++n)if(p[n]){failed=true;microkit_dbg_puts("DESKTOP_GPU|FAIL|private cache erasure\n");return false;}
    lock_erased+=bytes;revisions[i]=0;
    return upload(i,i<=DESKTOP_APP_COUNT?AW:DW,(struct gpu_rect){0,0,i<=DESKTOP_APP_COUNT?AW:DW,i<=DESKTOP_APP_COUNT?APP_MAX_PIXEL_HEIGHT:MAX_DH});
}
static void lock_completed_frame(void) {
    if(!lock_active||!lock_epoch)return;
    lock_targets|=1u<<(target-7);
    if(lock_stage==0){lock_stage=1;frame_pacer_request(&pacer,ticks());return;}
    lock_stage=2;
    if(lock_targets!=3){frame_pacer_request(&pacer,ticks());return;}
    struct trio_display_ack *a=(void*)lock_display_ack;uint32_t r=atomic_load(&a->revision);
    atomic_store(&a->revision,r+1);atomic_thread_fence(memory_order_seq_cst);
    a->epoch=lock_epoch;a->targets=3;a->erased_bytes=lock_erased;a->width=W;a->height=H;
    atomic_store_explicit(&a->revision,r+2,memory_order_release);
}
