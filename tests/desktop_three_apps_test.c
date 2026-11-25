#include <assert.h>
#include <stdio.h>
#include <string.h>
#define DESKTOP_APP_COUNT 3
#define DESKTOP_REAL_APPS
#define DESKTOP_SCALE 2u
#define SLINT_FRAME_WIDTH 2560u
#define SLINT_FRAME_HEIGHT 1600u
#define SLINT_FRAME_REGION_BYTES 0x1000000u
#include "../experiments/desktop-shell/compositor.c"
static void dynamic_input_notify(void) {}
static bool dynamic_control(struct input_event_record e) { (void)e; return false; }
static struct {struct surface_header h; uint8_t pixels[SLINT_FRAME_PIXEL_BYTES];} workspace;
static struct {slint_frame_header_t h; uint8_t pixels[APP_BYTES];} apps[3];
static uint8_t output[SLINT_FRAME_REGION_BYTES];
static uint32_t queues[3][1024], pointer[4], focus_word;
static struct desktop_queue shell_queue;
static struct shell_state shared;
static struct input_event_ring ring;
static uint8_t pixel(unsigned x,unsigned y) {return output[sizeof(slint_frame_header_t)+(y*2*2560+x*2)*3];}
static void key_event(unsigned code,unsigned value) {
    assert(input_ring_try_push(&ring,(struct input_event_record){.device=INPUT_DEVICE_KEYBOARD,.type=1,.code=code,.value=value}));input();
}
static void click_at(int x,int y) { pointer_x=x;pointer_y=y;pointer_event(1);pointer_event(0); }
int main(void) {
    _Static_assert(sizeof(struct shell_state)==64,"three-window ABI");
    input_a=(uintptr_t)queues[0];input_b=(uintptr_t)queues[1];input_c=(uintptr_t)queues[2];
    shell_input=(uintptr_t)&shell_queue;shell_state=(uintptr_t)&shared;focus_page=(uintptr_t)&focus_word;
    input_handoff=(uintptr_t)&ring;ui_pointer=(uintptr_t)pointer;output_frame=(uintptr_t)output;
    shell_surface=(uintptr_t)&workspace;surface_a=(uintptr_t)&apps[0];surface_b=(uintptr_t)&apps[1];surface_c=(uintptr_t)&apps[2];
    workspace.h=(struct surface_header){.revision=2,.width=2560,.height=1600,.stride=7680};
    memset(workspace.pixels,20,sizeof(workspace.pixels));dynamic_active=true;
    desktop.reserved=0x321;
    for(unsigned i=0;i<3;++i) {
        desktop.windows[i]=(struct shell_window){100,150,1,1};
        apps[i].h=(slint_frame_header_t){.revision=2,.width=1120,.height=1040,.stride_bytes=3360,.format=1};
        memset(apps[i].pixels,50+i*10,APP_BYTES);
    }
    for(unsigned id=1;id<=3;++id) {
        focus(id);assert(compose());assert(pixel(200,200)==40+id*10);
        assert(pixel(200,125)==20); /* Apps cannot paint trusted titlebars. */
        assert(shell_window_at(&desktop,200,200)==id);
        assert(shell_hit(&desktop,200,200)==id);
        assert(shell_hit(&desktop,200,125)==4);
    }
    focus(1);key_event(30,1);assert(key_owners[30]==1);
    focus(3);unsigned browser_head=queues[2][2],old_head=queues[0][2];
    key_event(30,2);assert(queues[0][2]==old_head && queues[2][2]==browser_head);
    key_event(30,0);
    assert(!key_owners[30] && queues[2][2]==browser_head); /* Release stays with original owner. */
    key_event(48,1);assert(key_owners[48]==3 && queues[2][2]==browser_head+1);
    release_owner(3);desktop.windows[2].visible=0;focus(0);key_event(48,0);
    assert(!key_owners[48]);assert(shell_window_at(&desktop,200,200)==1);
    assert(compose() && pixel(200,200)==50);
    desktop.windows[2].visible=1;focus(3);assert(compose() && pixel(200,200)==70);
    apps[2].h.width=2560;apps[2].h.revision=4;memset(apps[2].pixels,99,APP_BYTES);
    composition_dirty=true;assert(compose() && pixel(200,200)==70);
    /* No Slint callback runs here: dock focus precedes the next key event. */
    desktop.windows[2].visible=desktop.windows[2].open=0;focus(1);
    click_at(724,728);assert(desktop.focused==3 && desktop.windows[2].visible && desktop.windows[2].open);
    key_event(46,1);assert(key_owners[46]==3);key_event(46,0);
    click_at(590,128);assert(!desktop.windows[2].visible && desktop.windows[2].open && !desktop.focused);
    click_at(724,728);click_at(630,128);assert(!desktop.windows[2].visible && !desktop.windows[2].open);
    pointer_x=724;pointer_y=728;pointer_event(1);pointer_x=900;pointer_event(0);
    assert(!desktop.windows[2].visible); /* Dragging off cancels launch. */
    pointer_x=724;pointer_y=728;pointer_event(1);reset_input();pointer_event(0);
    assert(!desktop.windows[2].visible); /* Dropped input cancels launch. */
    puts("PASS three-app stacking, trusted chrome, ordered dock focus, focus isolation, retained windows and frame bounds");
}
