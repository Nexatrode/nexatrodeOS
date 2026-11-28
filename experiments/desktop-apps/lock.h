/* Curated three-app service restart boundary; only the opt-in disposable image. */
#include "lock_protocol.h"
#define ROOT_VSPACE (4UL<<58)
static uint64_t lock_epoch=1,activity_ms,vault_lease_until,generations,foreign_denials;
static bool device_locked=true,vault_failed;
static uint64_t scrubbed_bytes;
static unsigned lock_phase=TRIO_LOCKING;
#ifdef TRIO_STARTUP_TRACE
static uint64_t startup_generation,startup_at;
#endif
static bool trio_is_locked(void) {return device_locked;}
static uint32_t auth_sent;
static void print(const char *s) {microkit_dbg_puts(s);}
static void line(const char *s,uint64_t n) {
    char b[24];unsigned used=0;do{b[used++]='0'+n%10;n/=10;}while(n);
    print(s);while(used)microkit_dbg_putc(b[--used]);print("\n");
}
static void fail(const char *s,int e) {print("TRIO|FAIL|");print(s);line("|",e);for(;;)seL4_Yield();}
static void check(seL4_Error e,const char *s) {if(e)fail(s,e);}
static void copy(void *d,const void *s,size_t n) {memcpy(d,s,n);}
#define DYNAMIC_SIMPLEX_LIVE
#include "../desktop-simplex/session.h"
static uint64_t trio_now(void) {
    uint64_t t,f;
#if CONFIG_EXPORT_VCNT_USER
    asm volatile("isb; mrs %0,cntvct_el0; mrs %1,cntfrq_el0":"=r"(t),"=r"(f));
#elif CONFIG_EXPORT_PCNT_USER
    asm volatile("isb; mrs %0,cntpct_el0; mrs %1,cntfrq_el0":"=r"(t),"=r"(f));
#else
#error "desktop lock needs a monotonic counter"
#endif
    return t/f*1000+t%f*1000/f;
}
static void trio_publish(void) {
    struct trio_status *s=(void*)desktop_lock_state;uint32_t r=atomic_load(&s->revision);
    atomic_store(&s->revision,r+1);atomic_thread_fence(memory_order_seq_cst);
    s->locked=device_locked;s->epoch=lock_epoch;s->activity_ms=activity_ms;s->idle_ms=LOCK_IDLE_MS;
    s->phase=lock_phase;s->failed=vault_failed||session_failed;s->scrubbed=scrubbed_bytes;
    s->generation=generations;s->foreign_denials=foreign_denials;
    atomic_store_explicit(&s->revision,r+2,memory_order_release);
    microkit_notify(8);microkit_notify(2);microkit_notify(10);
}
static bool trio_vault(unsigned label,unsigned phase,bool erased) {
    microkit_mr_set(0,lock_epoch);
    microkit_msginfo r=microkit_ppcall(13,microkit_msginfo_new(label,1));
    bool ok=microkit_msginfo_get_label(r)==0x300&&microkit_msginfo_get_count(r)==3&&
        microkit_mr_get(0)==lock_epoch&&microkit_mr_get(1)==phase&&microkit_mr_get(2)==erased;
    if(!ok)vault_failed=true;
    return ok;
}
static void trio_drop_input(void) {
    /* App queues are part of the wiped group. Retire the manager's producers too. */
    memset(key_owners,0,sizeof(key_owners));pointer_owner=drag_owner=chrome_pending=0;
    memset(app_sent,0,sizeof(app_sent));memset(app_reset,0,sizeof(app_reset));
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i){sent[i]=0;reset_pending[i]=false;}
    emit(DESKTOP_SHELL_OWNER,(struct desktop_event){.kind=DESKTOP_RESET});
    struct input_event_ring *ring=(void*)input_handoff;
    driver_tail=__atomic_load_n(&ring->head,__ATOMIC_ACQUIRE);
    __atomic_store_n(&ring->tail,driver_tail,__ATOMIC_RELEASE);
    struct desktop_queue *q=(void*)auth_input;auth_sent=0;memset(q,0,sizeof(*q));
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i){desktop.windows[i].open=desktop.windows[i].visible=0;app_revision[i]=0;}
    desktop.focused=desktop.top=0;publish_state();
}
static bool trio_covered(void) {
    const struct trio_display_ack *a=(void*)lock_display_ack;uint32_t r=atomic_load_explicit(&a->revision,memory_order_acquire);
    uint64_t epoch=a->epoch;unsigned targets=a->targets,width=a->width,height=a->height;atomic_thread_fence(memory_order_acquire);
    return r&&!(r&1)&&r==atomic_load(&a->revision)&&epoch==lock_epoch&&targets==3&&
        width==SHELL_PIXEL_WIDTH&&height==SHELL_PIXEL_HEIGHT;
}
static void trio_resized(void) {
    if(device_locked&&lock_phase!=TRIO_FAILED){lock_phase=TRIO_LOCKING;trio_publish();}
}
static void trio_lock(bool force) {
    if(device_locked&&!force)return;
    device_locked=true;dynamic_active=false;lock_phase=TRIO_LOCKING;++lock_epoch;vault_lease_until=0;
    simplex_session_suspend();trio_drop_input();trio_publish();
#ifdef SESSION_MAPS_REGS_ADDRESS
    /* VirtIO reset acknowledges that queue/DMA activity has stopped. Reset
     * before erasing any descriptor, store state or shared ciphertext page. */
    volatile uint32_t *status=(void*)(SESSION_MAPS_REGS_ADDRESS+0x70);
    *status=0;__atomic_thread_fence(__ATOMIC_SEQ_CST);
    if(*status)fail("Maps disk reset incomplete",0);
#endif
    (void)trio_vault(0x453,0,true);
    simplex_session_scrub();scrubbed_bytes=0;
    for(unsigned i=0;i<SESSION_RANGE_COUNT;++i)scrubbed_bytes+=session_ranges[i].bytes;
    if(session_failed||vault_failed)lock_phase=TRIO_FAILED;
    trio_publish();
}
static void trio_tick(void) {
    uint64_t now=trio_now();
#ifdef TRIO_STARTUP_TRACE
    if(!device_locked&&startup_generation!=generations&&now-startup_at>=2000){
        startup_generation=generations;
        const unsigned ids[]={32,34,38,39};
        for(unsigned i=0;i<sizeof(ids)/sizeof(ids[0]);++i){
            seL4_UserContext c={0};
            check(seL4_TCB_ReadRegisters(BASE_TCB_CAP+ids[i],false,0,sizeof(c)/sizeof(seL4_Word),&c),"startup PC read");
            line("TRIO|STARTUP_CHILD=",ids[i]);line("TRIO|STARTUP_PC=",c.pc);
        }
    }
#endif
    if(device_locked&&vault_lease_until&&now>=vault_lease_until)trio_lock(true);
    if(!device_locked&&now-activity_ms>=LOCK_IDLE_MS)trio_lock(false);
    if(device_locked&&lock_phase==TRIO_LOCKING&&trio_covered()){
        lock_phase=(session_failed||vault_failed)?TRIO_FAILED:TRIO_LOCKED;
        trio_publish();line("TRIO|LOCK_COMPLETE|epoch=",lock_epoch);
    }
    microkit_notify(8);
    microkit_mr_set(0,250000000);microkit_ppcall(12,microkit_msginfo_new(1,1));
}
static void trio_boot(void) {
    simplex_session_boot();
    (void)trio_vault(0x453,0,true);
    for(unsigned i=0;i<SESSION_RANGE_COUNT;++i)scrubbed_bytes+=session_ranges[i].bytes;
    if(session_failed||vault_failed)lock_phase=TRIO_FAILED;
    trio_drop_input();trio_publish();trio_tick();
    print("TRIO|BOOT_LOCKED\n");
}
static void trio_auth_emit(struct desktop_event e) {
    struct desktop_queue *q=(void*)auth_input;uint32_t tail=__atomic_load_n(&q->tail,__ATOMIC_ACQUIRE);
    if(auth_sent-tail>=DESKTOP_EVENT_CAPACITY)return;
    q->events[auth_sent++%DESKTOP_EVENT_CAPACITY]=e;
    __atomic_store_n(&q->head,auth_sent,__ATOMIC_RELEASE);microkit_notify(8);
}
static bool trio_input(struct input_event_record e) {
    if(e.device==INPUT_DEVICE_KEYBOARD&&e.type==1&&e.code==67){if(e.value==1)trio_lock(false);return true;}
    if(!device_locked){activity_ms=trio_now();return false;}
    if(e.device==INPUT_DEVICE_KEYBOARD&&e.type==1&&e.code<=DESKTOP_KEY_MAX&&e.value<=2)
        trio_auth_emit((struct desktop_event){.kind=DESKTOP_KEY,.code=e.code,.value=e.value});
    else if(e.device==INPUT_DEVICE_TABLET){
        if(e.type==3&&e.code<=1&&e.value<=32767){
            if(!e.code)pointer_x=e.value*(SHELL_WIDTH-1)/32767;else pointer_y=e.value*(SHELL_HEIGHT-1)/32767;
        }
        struct trio_auth_box b=trio_auth_box(SHELL_PIXEL_WIDTH,SHELL_PIXEL_HEIGHT);
        int x=((int)pointer_x*2-b.left)*(int)TRIO_AUTH_W/b.width;
        int y=((int)pointer_y*2-b.top)*(int)TRIO_AUTH_H/b.height;
        if((e.type==3&&e.code<=1)||(e.type==1&&e.code==272&&e.value<=1))
            trio_auth_emit((struct desktop_event){.kind=DESKTOP_POINTER,.value=e.type==3?2:e.value,.x=x,.y=y});
    }
    return true;
}
static microkit_msginfo trio_request(microkit_channel ch,microkit_msginfo m) {
    unsigned label=microkit_msginfo_get_label(m),n=microkit_msginfo_get_count(m);
    if(ch!=TRIO_AUTH_CHANNEL||seL4_MessageInfo_get_extraCaps(m)){
        ++foreign_denials;trio_publish();return microkit_msginfo_new(1,0);
    }
    if(label==LOCK_STATUS&&!n){
        microkit_mr_set(0,device_locked);microkit_mr_set(1,lock_epoch);microkit_mr_set(2,LOCK_IDLE_MS);
        microkit_mr_set(3,vault_failed||session_failed);return microkit_msginfo_new(0,4);
    }
    if(label==LOCK_REQUEST&&!n){trio_lock(false);return microkit_msginfo_new(0,0);}
    if(!device_locked||n!=1||microkit_mr_get(0)!=lock_epoch)return microkit_msginfo_new(1,0);
    if(label==LOCK_ABORT){trio_lock(true);return microkit_msginfo_new(0,0);}
    if(label==LOCK_PREPARE&&lock_phase==TRIO_LOCKED&&trio_covered()&&!vault_failed&&!session_failed&&!vault_lease_until){
        if(!trio_vault(0x456,1,true)){trio_lock(true);return microkit_msginfo_new(2,0);}
        vault_lease_until=trio_now()+LOCK_VAULT_LEASE_MS;return microkit_msginfo_new(0,0);
    }
    if(label==LOCK_UNLOCK&&lock_phase==TRIO_LOCKED&&trio_covered()&&!vault_failed&&!session_failed&&vault_lease_until&&trio_now()<vault_lease_until){
        if(!trio_vault(0x455,3,false)||!simplex_session_start()){trio_lock(true);return microkit_msginfo_new(2,0);}
        vault_lease_until=0;device_locked=false;dynamic_active=true;lock_phase=TRIO_OPEN;++generations;activity_ms=trio_now();
#ifdef TRIO_STARTUP_TRACE
        startup_at=activity_ms;
#endif
        trio_drop_input();
        /* Do not enqueue an old focus or key into freshly constructed clients. */
        desktop.windows[0].open=desktop.windows[0].visible=1;
        desktop.windows[1].open=desktop.windows[1].visible=1;
        focus(1);trio_publish();line("TRIO|UNLOCKED|generation=",generations);return microkit_msginfo_new(0,0);
    }
    return microkit_msginfo_new(1,0);
}
