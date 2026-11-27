/* Static desktop app desktop experiment. This does not grant dynamic lifecycle or
 * lock/eviction guarantees: minimizing/closing hides a window, services live on. */
#include <stddef.h>
#include <stdint.h>
void *memcpy(void *to, const void *from, size_t n)
{
    for (size_t i=0;i<n;++i)((uint8_t *)to)[i]=((const uint8_t *)from)[i];
    return to;
}
void *memset(void *to, int value, size_t n)
{
    for (size_t i=0;i<n;++i)((uint8_t *)to)[i]=(uint8_t)value;
    return to;
}
#define DESKTOP_REAL_APPS
#define init compositor_init
#define notified compositor_notified
#define fault compositor_fault
#include "../desktop-shell/compositor.c"
#undef init
#undef notified
#undef fault
#ifdef DESKTOP_DYNAMIC
#include "../desktop-gpu/display_protocol.h"
#ifdef DESKTOP_TRIO_LOCK
static bool trio_is_locked(void);
static void trio_resized(void);
#endif
uintptr_t desktop_display_mode,desktop_display_request;
static uint32_t resize_serial;
static void resize_display(bool initial) {
    struct display_mode next={0};
    if(initial){display_dimensions(DISPLAY_DEFAULT_W,DISPLAY_DEFAULT_H,&next);}
    else {
        const struct display_request *r=(void*)desktop_display_request;
        uint32_t rev=__atomic_load_n(&r->revision,__ATOMIC_ACQUIRE);struct display_request copy=*r;
        __atomic_thread_fence(__ATOMIC_ACQUIRE);
        if(!rev||(rev&1)||rev!=__atomic_load_n(&r->revision,__ATOMIC_ACQUIRE)||!copy.serial||copy.serial==resize_serial||!copy.width||!copy.height||copy.width>16384||copy.height>16384)return;
        display_dimensions(copy.width,copy.height,&next);next.serial=copy.serial;
    }
    bool changed=next.width/2!=desktop_width||next.height/2!=desktop_height||next.app_height!=desktop_app_height;
    desktop_width=next.width/2;desktop_height=next.height/2;desktop_app_height=next.app_height;
    struct display_mode *out=(void*)desktop_display_mode;uint32_t rev=__atomic_load_n(&out->revision,__ATOMIC_RELAXED);
    __atomic_store_n(&out->revision,rev+1,__ATOMIC_RELEASE);__atomic_thread_fence(__ATOMIC_SEQ_CST);
    out->serial=next.serial;out->width=next.width;out->height=next.height;out->scale=2;out->app_height=next.app_height;
    __atomic_store_n(&out->revision,rev+2,__ATOMIC_RELEASE);resize_serial=next.serial;
    if(!initial){
        bool route=true;
#ifdef DESKTOP_TRIO_LOCK
        route=!trio_is_locked();
#endif
        if(changed){if(route)reset_input();for(unsigned i=0;i<DESKTOP_APP_COUNT;++i)shell_position(&desktop.windows[i],desktop.windows[i].x,desktop.windows[i].y);pointer_x=shell_clamp(pointer_x,0,desktop_width-1);pointer_y=shell_clamp(pointer_y,0,desktop_height-1);}
        publish_state();microkit_notify(2);if(route)for(unsigned i=1;i<=DESKTOP_APP_COUNT;++i)input_notify(i);
#ifdef DESKTOP_TRIO_LOCK
        if(changed)trio_resized();
#endif
    }
}
#endif
#ifdef DESKTOP_TRIO_LOCK
uintptr_t desktop_lock_state,lock_display_ack,auth_input;
#include "lock.h"
#endif
static void dynamic_input_notify(void) { microkit_notify(24); }
static bool dynamic_control(struct input_event_record e) { (void)e; return false; }
void init(void)
{
#ifdef DESKTOP_DYNAMIC
    resize_display(true);
#endif
    dynamic_active = true; compositor_init();
#ifdef DESKTOP_TRIO_LOCK
    desktop.windows[2]=(struct shell_window){340,152,0,0};desktop.reserved=0x321;
    dynamic_active=false;trio_boot();return;
#endif
    desktop.windows[0].open = desktop.windows[0].visible = 1;
    desktop.windows[1].open = desktop.windows[1].visible = 1;
    #if DESKTOP_APP_COUNT == 3
    desktop.windows[2]=(struct shell_window){340,152,0,0};
    desktop.reserved=0x321;
#endif
    focus(1);
    #if DESKTOP_APP_COUNT == 3
    microkit_dbg_puts("DESKTOP_APPS|READY|SimpleX+NexaWal+NetSurf|static-disposable\n");
#else
    microkit_dbg_puts("DESKTOP_APPS|READY|SimpleX+NexaWal|static-disposable\n");
#endif
}
void notified(microkit_channel channel)
{
#ifdef DESKTOP_TRIO_LOCK
    if(channel==12)trio_tick();
#endif
#ifdef DESKTOP_DYNAMIC
    if(channel==31)resize_display(false);
#endif
    for (unsigned i = 0; i < DESKTOP_APP_COUNT; ++i) {
#ifdef DESKTOP_TRIO_LOCK
        if(device_locked)break;
#endif
        uint32_t *q = (void *)app_input(i+1);
        if (__atomic_exchange_n(&q[1], 0, __ATOMIC_ACQ_REL)) input_notify(i + 1);
    }
    compositor_notified(channel);
}
microkit_msginfo protected(microkit_channel channel, microkit_msginfo m)
{
#ifdef DESKTOP_TRIO_LOCK
    if(channel==TRIO_AUTH_CHANNEL||channel==TRIO_PROBE_CHANNEL)return trio_request(channel,m);
    if(device_locked)return microkit_msginfo_new(1,0);
#endif
    if (channel != 61 || microkit_msginfo_get_label(m) != SHELL_REQUEST ||
        microkit_msginfo_get_count(m) != 4 || seL4_MessageInfo_get_extraCaps(m)) return microkit_msginfo_new(1, 0);
    uint64_t command = microkit_mr_get(0), id = microkit_mr_get(1);
    if (id < 1 || id > DESKTOP_APP_COUNT || command < 1 || command > 5) return microkit_msginfo_new(1, 0);
    struct shell_window *w = &desktop.windows[id - 1];
    if (command == SHELL_APP_LAUNCH) { w->open = w->visible = 1; focus(id); }
    else if (command == SHELL_APP_MINIMIZE || command == SHELL_APP_CLOSE) {
        w->visible = 0; if (command == SHELL_APP_CLOSE) w->open = 0;
        release_owner(id); if (desktop.focused == id) focus(0);
    } else if (command == SHELL_APP_MOVE) {
        int64_t x = (int64_t)microkit_mr_get(2), y = (int64_t)microkit_mr_get(3);
        if (!w->visible || x < INT32_MIN || x > INT32_MAX || y < INT32_MIN || y > INT32_MAX) return microkit_msginfo_new(1, 0);
        shell_position(w, x, y);
    } else { if (!w->visible) return microkit_msginfo_new(1, 0); focus(id); }
    publish_state(); compose();
    return microkit_msginfo_new(0, 0);
}
seL4_Bool fault(microkit_child child, microkit_msginfo info, microkit_msginfo *reply)
{
#ifdef DESKTOP_TRIO_LOCK
    (void)child;(void)info;(void)reply;trio_lock(true);session_failed=vault_failed=true;lock_phase=TRIO_FAILED;trio_publish();return seL4_False;
#else
    return compositor_fault(child, info, reply);
#endif
}
