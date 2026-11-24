#include <assert.h>
#include <stdio.h>
#include <string.h>
#ifndef DESKTOP_SCALE
#define DESKTOP_SCALE 1u
#endif
#define SLINT_FRAME_WIDTH (1280u * DESKTOP_SCALE)
#define SLINT_FRAME_HEIGHT (800u * DESKTOP_SCALE)
#define SLINT_FRAME_REGION_BYTES 0x1000000u
#include "../experiments/desktop-shell/compositor.c"
static void dynamic_input_notify(void) {}
static bool dynamic_control(struct input_event_record e) { (void)e; return false; }
struct frame { struct surface_header h; uint8_t pixels[SLINT_FRAME_PIXEL_BYTES]; };
static struct frame workspace, first, second;
static uint8_t output[SLINT_FRAME_REGION_BYTES];
static struct shell_state shared_state;
static struct desktop_queue q[3];
static uint32_t focus_word;
static uint8_t visible(unsigned x, unsigned y) { return output[sizeof(slint_frame_header_t) + (y*DESKTOP_SCALE*SLINT_FRAME_WIDTH+x*DESKTOP_SCALE)*3]; }
static void offer(struct frame *f, unsigned w, unsigned h, uint8_t color) {
    f->h = (struct surface_header){.revision=2, .width=w, .height=h, .stride=w*3};
    memset(f->pixels,color,w*h*3);
}
int main(void) {
    shell_surface=(uintptr_t)&workspace; shell_state=(uintptr_t)&shared_state; shell_input=(uintptr_t)&q[2];
    surface_a=(uintptr_t)&first; surface_b=(uintptr_t)&second; output_frame=(uintptr_t)output;
    input_a=(uintptr_t)&q[0]; input_b=(uintptr_t)&q[1]; focus_page=(uintptr_t)&focus_word;
    offer(&workspace,SHELL_PIXEL_WIDTH,SHELL_PIXEL_HEIGHT,20); offer(&first,APP_PIXEL_WIDTH,APP_PIXEL_HEIGHT,99); offer(&second,APP_PIXEL_WIDTH,APP_PIXEL_HEIGHT,42);
    init(); assert(compose());
    assert(visible(100,200)==20);
    dynamic_active=true; desktop.windows[0].open=desktop.windows[0].visible=1;
    desktop.windows[1]=(struct shell_window){100,150,1,1}; publish_state(); assert(compose());
    assert(visible(200,200)==42); assert(visible(200,140)==20); /* Higher trusted chrome occludes lower app. */
    focus(1); assert(compose()); assert(visible(200,200)==99);
    desktop.windows[0].visible=0; focus(0); assert(compose()); assert(visible(200,200)==42);
    desktop.windows[0].visible=1; focus(1); assert(compose());
    /* Reject malformed frames and retain the previously validated snapshot. */
    first.h.stride=0; atomic_store(&first.h.revision,4); memset(first.pixels,3,APP_BYTES);
    composition_dirty=true; assert(compose()); assert(visible(200,200)==99);
    key_owners[29]=2; pointer_owner=2; release_owner(2);
    assert(!key_owners[29] && !pointer_owner);
    key_owners[30]=1; key_owners[48]=2; pointer_owner=1;
    emit(1,(struct desktop_event){.kind=DESKTOP_KEY,.code=30,.value=1});
    discard_dynamic_surface(); surface_a=input_a=0;
    assert(!dynamic_active && !key_owners[30] && key_owners[48]==2 && !pointer_owner);
    assert(!desktop.windows[0].visible && !app_revision[0] && !sent[0]);
    assert(compose()); assert(visible(200,200)==42);
    emit(1,(struct desktop_event){.kind=DESKTOP_KEY}); /* Absent app queue is never dereferenced. */
    for (size_t i=0;i<APP_BYTES;++i) assert(!app_pixels[0][i]);
    for (size_t i=0;i<sizeof(candidate);++i) assert(!candidate[i]);
    puts("PASS landscape composition, trusted chrome, focus, malformed frames and closed-process cleanup");
}
