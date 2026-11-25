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
static void dynamic_input_notify(void) { microkit_notify(24); }
static bool dynamic_control(struct input_event_record e) { (void)e; return false; }
void init(void)
{
    dynamic_active = true; compositor_init();
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
    for (unsigned i = 0; i < DESKTOP_APP_COUNT; ++i) {
        uint32_t *q = (void *)app_input(i+1);
        if (__atomic_exchange_n(&q[1], 0, __ATOMIC_ACQ_REL)) input_notify(i + 1);
    }
    compositor_notified(channel);
}
microkit_msginfo protected(microkit_channel channel, microkit_msginfo m)
{
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
{ return compositor_fault(child, info, reply); }
