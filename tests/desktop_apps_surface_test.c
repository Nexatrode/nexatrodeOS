#include <assert.h>
#include <stdio.h>
#include <string.h>
#define DESKTOP_REAL_APPS
#define DESKTOP_SCALE 2u
#define SLINT_FRAME_WIDTH 2560u
#define SLINT_FRAME_HEIGHT 1600u
#define SLINT_FRAME_REGION_BYTES 0x1000000u
#include "../experiments/desktop-shell/compositor.c"
static void dynamic_input_notify(void) {}
static bool dynamic_control(struct input_event_record e) { (void)e; return false; }
static uint32_t queues[2][1024];
static struct {slint_frame_header_t h; uint8_t rgb[APP_BYTES];} app;
int main(void)
{
    dynamic_active=true;input_a=(uintptr_t)queues[0];input_b=(uintptr_t)queues[1];
    app_emit(1,(struct desktop_event){.kind=DESKTOP_POINTER,.value=1,.x=123,.y=234});
    assert(queues[0][2]==1 && queues[1][2]==0);
    assert(queues[0][4]==(INPUT_DEVICE_TABLET|(1u<<16)) && queues[0][5]==272 && queues[0][6]==1);
    assert(queues[0][7]==(246u|(468u<<16)));
    app_emit(2,(struct desktop_event){.kind=DESKTOP_KEY,.code=30,.value=1});
    assert(queues[1][2]==1 && queues[1][5]==30);
    for(unsigned i=1;i<224;++i)app_emit(1,(struct desktop_event){.kind=DESKTOP_KEY,.code=30,.value=1});
    app_emit(1,(struct desktop_event){.kind=DESKTOP_KEY,.code=31,.value=1});
    assert(queues[0][2]==224 && app_reset[0]);
    queues[0][3]=224;
    app_emit(1,(struct desktop_event){.kind=DESKTOP_KEY,.code=32,.value=1});
    assert(queues[0][2]==226 && queues[0][4]==2 && queues[0][9]==32 && !app_reset[0]);
    app.h=(slint_frame_header_t){.revision=2,.width=APP_PIXEL_WIDTH,.height=APP_PIXEL_HEIGHT,.stride_bytes=APP_PIXEL_WIDTH*3,.format=1};
    memset(app.rgb,42,sizeof(app.rgb));uint32_t rev=0;
    assert(snapshot_app((uintptr_t)&app,app_pixels[0],&rev) && rev==2 && app_pixels[0][0]==42);
    app.h.width=1080;app.h.revision=4;app.rgb[0]=99;
    assert(!snapshot_app((uintptr_t)&app,app_pixels[0],&rev) && rev==2 && app_pixels[0][0]==42);
    app_emit(2,(struct desktop_event){.kind=DESKTOP_POINTER,.value=0,.x=-2,.y=-3});
    assert((int16_t)(queues[1][11]&0xffff)==-4 && (int16_t)(queues[1][11]>>16)==-6);
    release_owner(1);
    puts("PASS desktop phone-frame validation, input isolation, scaling and overflow reset");
}
