/* Exercise the real manager resize handler without device MMIO. */
#include <assert.h>
#include <stdio.h>
#include "surface_stub/microkit.h"
static inline unsigned microkit_msginfo_get_count(unsigned m) {(void)m;return 0;}
static inline unsigned microkit_msginfo_new(unsigned a,unsigned b) {(void)b;return a;}
static inline unsigned seL4_MessageInfo_get_extraCaps(unsigned m) {(void)m;return 0;}
#define DESKTOP_DYNAMIC
#define DESKTOP_VIRGL
#define DESKTOP_APP_COUNT 3
#define DESKTOP_SCALE 2u
#define SLINT_FRAME_WIDTH 2560u
#define SLINT_FRAME_HEIGHT 1600u
#define SLINT_FRAME_REGION_BYTES 0x1000000u
#include "../experiments/desktop-apps/manager.c"
static struct display_mode mode;
static struct display_request request;
static struct shell_state state;
static struct desktop_queue queue;
static uint32_t app_queue[3][1024];
static void change(unsigned serial,unsigned width,unsigned height) {
    request=(struct display_request){2,serial,width,height};resize_display(false);
}
int main(void) {
    desktop_display_mode=(uintptr_t)&mode;desktop_display_request=(uintptr_t)&request;
    shell_state=(uintptr_t)&state;shell_input=(uintptr_t)&queue;
    input_a=(uintptr_t)app_queue[0];input_b=(uintptr_t)app_queue[1];input_c=(uintptr_t)app_queue[2];
    dynamic_active=true;resize_display(true);assert(mode.width==2560&&mode.height==1600&&mode.app_height==520);
    desktop.windows[0]=(struct shell_window){700,150,1,1};pointer_owner=1;drag_owner=1;key_owners[30]=1;
    change(1,1600,1200);assert(mode.width==1600&&mode.height==1200&&mode.app_height==380);
    assert(!pointer_owner&&!drag_owner&&!key_owners[30]);
    assert(state.windows[0].x==228&&state.windows[0].y==92);
    assert(chrome_at(400,520)==(2<<8|SHELL_APP_LAUNCH));
    unsigned revision=mode.revision;change(1,3840,2160);assert(mode.revision==revision); /* duplicate serial */
    change(2,3841,2161);assert(mode.width==3840&&mode.height==2160&&mode.app_height==520);
    change(3,1,1);assert(mode.width==1600&&mode.height==1200);
    revision=mode.revision;change(4,0,1500);assert(mode.revision==revision);
    change(5,16385,1500);assert(mode.revision==revision);
    request=(struct display_request){3,6,2560,1600};resize_display(false);assert(mode.revision==revision); /* in-progress */
    change(7,2559,1599);assert(mode.width==2558&&mode.height==1598);
    for(unsigned i=8;i<1008;++i){change(i,i&1?1600:3840,i&1?1200:2160);assert(!(mode.revision&1));assert(state.windows[0].x>=12&&state.windows[0].x+560<=mode.width/2-12);assert(state.windows[0].y+mode.app_height<=mode.height/2-128);}
    uint8_t payload[28]={1};payload[9]=4;payload[13]=8;payload[16]=32;
    uint32_t w=0,h=0;assert(display_monitor_request(payload,28,&w,&h)&&w==2048&&h==1024);
    assert(!display_monitor_request(payload,27,&w,&h));
    payload[20]=1;assert(!display_monitor_request(payload,28,&w,&h));payload[20]=0;
    payload[0]=2;assert(!display_monitor_request(payload,28,&w,&h));payload[0]=1;
    payload[16]=0;assert(!display_monitor_request(payload,28,&w,&h));payload[16]=32;
    payload[15]=1;assert(!display_monitor_request(payload,28,&w,&h));
    puts("PASS resize bounds, malformed and duplicate requests, shrink clamps, dock hit testing, held-input cancellation, 1000 alternating modes");
}
