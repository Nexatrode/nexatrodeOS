/* Actual trusted router/compositor, with fixture-owned RAM and notify mocks. */
#include <assert.h>
#include <stdatomic.h>
#include <stdio.h>
#include <string.h>
#include "../experiments/desktop-surfaces/input.h"
#define SLINT_FRAME_WIDTH 1080u
#define SLINT_FRAME_HEIGHT 2340u
#define SLINT_FRAME_STRIDE_BYTES (1080u*3)
#define SLINT_FRAME_PIXEL_BYTES (SLINT_FRAME_STRIDE_BYTES*2340u)
#define SLINT_FRAME_FORMAT_RGB8 1u
typedef struct { _Atomic uint32_t revision;uint32_t width,height,stride_bytes,format; _Atomic uint32_t damage_y,damage_height;} slint_frame_header_t;
static struct {slint_frame_header_t header;uint8_t pixels[SLINT_FRAME_PIXEL_BYTES];} front_source,output;
static uint32_t focus_words[1024];
#define FRONTEND_FRAME_ADDRESS ((uintptr_t)&front_source)
#define FRONTEND_FOCUS_ADDRESS ((uintptr_t)focus_words)
static uintptr_t output_frame=(uintptr_t)&output;
static uint8_t composed_pixels[SLINT_FRAME_PIXEL_BYTES];
static uint32_t frontend_revision;
static bool device_locked=true;
static struct {uint64_t epoch,active;} state;
static typeof(state) *session_state=&state;
static unsigned notifications[19],compositions,overflows;
static uint64_t activity_ms;
static uint64_t lock_now(void){return 12345;}
static void microkit_notify(unsigned n){assert(n==2 || n==18);notifications[n]++;}
static void print(const char *s){assert(!strcmp(s,"SIMPLEX_FRONTEND|INPUT_OVERFLOW\n"));overflows++;}
static bool compose(void){compositions++;return true;}
#include "../experiments/desktop-simplex/frontend.h"
int main(void){
    struct input_event_record key={.device=0,.type=1,.code=30,.value=1};
    assert(!frontend_input(key));frontend_notify();assert(!compositions && !notifications[18]);
    device_locked=false;assert(!frontend_input(key));state.active=1;
    assert(frontend_input(key));assert(focus_words[2]==1 && focus_words[0]==1 && focus_words[4]==0x10000 && focus_words[5]==30 && focus_words[6]==1);
    assert(activity_ms==12345 && notifications[18]==1);
    key.reserved=1;frontend_input(key);assert(focus_words[2]==1);key.reserved=0;
    struct input_event_record pos={.device=1,.type=3,.code=0,.value=32767};frontend_input(pos);
    pos.code=1;frontend_input(pos);assert(focus_words[4+2*4+3]==(1079u|(2339u<<16)));
    pos.value=32768;frontend_input(pos);assert(focus_words[2]==3);
    while(focus_words[2]<224)frontend_input(key);
    frontend_input(key);assert(overflows==1 && focus_words[2]==224);
    focus_words[3]=1;frontend_input(key);assert(focus_words[2]==225);
    focus_words[1]=1;frontend_notify();assert(focus_words[1]==0 && compositions==1);
    device_locked=true;focus_words[1]=1;frontend_notify();assert(focus_words[1]==1 && compositions==1);
    front_source.header=(slint_frame_header_t){.revision=2,.width=1080,.height=2340,.stride_bytes=3240,.format=1};
    memset(front_source.pixels,0x39,sizeof(front_source.pixels));frontend_compose();
    assert(output.header.revision==2 && frontend_revision==2 && output.pixels[0]==0x39 && output.pixels[sizeof(output.pixels)-1]==0x39);
    frontend_compose();assert(output.header.revision==2); // unchanged frame is not copied/presented again
    front_source.header.revision=4;front_source.pixels[0]=0x51;frontend_compose();assert(output.header.revision==4 && output.pixels[0]==0x51);
    puts("PASS full frontend: locked input denial, bounded ring, coordinates, wake gate and unchanged-frame scheduling");
}
