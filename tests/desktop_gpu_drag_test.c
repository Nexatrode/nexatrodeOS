#include <assert.h>
#include <stdio.h>
#include <string.h>
#define DESKTOP_VIRGL
#define DESKTOP_REAL_APPS
#define DESKTOP_APP_COUNT 3
#define DESKTOP_SCALE 2u
#define SLINT_FRAME_WIDTH 2560u
#define SLINT_FRAME_HEIGHT 1600u
#define SLINT_FRAME_REGION_BYTES 0x1000000u
#include "../experiments/desktop-shell/compositor.c"
static void dynamic_input_notify(void) {}
static bool dynamic_control(struct input_event_record e) {(void)e;return false;}
static struct shell_state shared;
static struct desktop_queue shell_events;
static uint32_t app_events[3][1024];
int main(void) {
    shell_state=(uintptr_t)&shared;shell_input=(uintptr_t)&shell_events;
    input_a=(uintptr_t)app_events[0];input_b=(uintptr_t)app_events[1];input_c=(uintptr_t)app_events[2];
    dynamic_active=true;desktop.reserved=0x321;desktop.windows[0]=(struct shell_window){68,126,1,1};
    pointer_x=168;pointer_y=102;pointer_event(1);
    assert(drag_owner==1 && pointer_owner==DESKTOP_SHELL_OWNER);
    uint32_t before=shared.revision;
    pointer_x+=100;pointer_event(2);
    assert(desktop.windows[0].x==168 && shared.windows[0].x==168 && shared.revision>before);
    assert(shell_events.head==0); /* No delayed Slint drag callback owns geometry. */
    pointer_x=30000;pointer_y=30000;pointer_event(2);
    assert(desktop.windows[0].x==708 && desktop.windows[0].y==152);
    pointer_event(0);assert(!drag_owner && !pointer_owner);
    pointer_x=808;pointer_y=128;pointer_event(1);assert(drag_owner==1);
    reset_input();assert(!drag_owner && !pointer_owner);
    puts("PASS live held-button GPU drag, geometry publication, clamp, release and input-reset cancellation");
}
