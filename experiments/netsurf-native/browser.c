/* SPDX-License-Identifier: GPL-2.0-only
 * Standalone NetSurf UI protection domain. No filesystem or raw-network grants.
 */
#include <microkit.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "native.h"
#include "native_page.h"
#include "native_reflow_guard.h"
#include "native_dom_work.h"
#ifdef NATIVE_HTTPS
#include "https.h"
#endif
#include "netsurf/netsurf.h"
#include "netsurf/browser.h"
#include "netsurf/browser_window.h"
#include "netsurf/content.h"
#include "netsurf/keypress.h"
#include "netsurf/window.h"
#include "netsurf/misc.h"
#include "netsurf/fetch.h"
#include "desktop/browser_history.h"
#include "content/hlcache.h"
#include "utils/nsoption.h"
#include "utils/nsurl.h"
#include "utils/messages.h"
#include "../../tests/input/input_handoff.h"
#include "../../tests/input/slint_frame.h"

uintptr_t browser_frame, input_handoff, ui_focus, ui_pointer;
struct gui_window { struct browser_window *bw; int scroll, width, height, caret_x, caret_y, caret_h; };
static struct gui_window window;
#ifdef NATIVE_DISPLAY_RESIZE
#include "../desktop-gpu/display_protocol.h"
uintptr_t desktop_display_mode;
int native_view_height=520;
static void resize_viewport(void) {
    const struct display_mode *m=(void*)desktop_display_mode;uint32_t revision=__atomic_load_n(&m->revision,__ATOMIC_ACQUIRE),height=m->app_height;
    __atomic_thread_fence(__ATOMIC_ACQUIRE);
    if(!revision||(revision&1)||revision!=__atomic_load_n(&m->revision,__ATOMIC_ACQUIRE)||height<360||height>520||height==(unsigned)native_view_height)return;
    native_view_height=height;if(window.bw)browser_window_schedule_reformat(window.bw);native_ui_status_changed();
}
#endif
static bool dirty=true, shift, mouse_down, loaded;
static unsigned frames, navigations;
static int mouse_x, mouse_y;
static uint32_t input_tail;
#ifndef NATIVE_DESKTOP
static uint32_t dropped;
#endif
static bool editing;
static char typed_url[2048];
static char address[2048] = "resource:index.html";
void native_ui_status_changed(void){dirty=true;}
static void reload_page(void);
static void stop_page(void);
struct task { bool live; uint64_t due; void (*function)(void*); void *data; };
static struct task tasks[128];

static nserror schedule(int delay,void (*function)(void*),void *data) {
    struct task *slot=NULL;
    for(unsigned i=0;i<128;++i){
        if(tasks[i].live && tasks[i].function==function && tasks[i].data==data){slot=&tasks[i];break;}
        if(!tasks[i].live && !slot)slot=&tasks[i];
    }
    if(!slot)return delay<0?NSERROR_OK:NSERROR_NOMEM;
    slot->live=delay>=0;slot->function=function;slot->data=data;
    slot->due=native_ms()+(delay>0?(unsigned)delay:0);return NSERROR_OK;
}
static void arm(void) {
    uint64_t now=native_ms(),next=now+1000;
    for(unsigned i=0;i<128;++i)if(tasks[i].live && tasks[i].due<next)next=tasks[i].due;
    uint64_t delay=next>now?next-now:1;
    microkit_mr_set(0,delay*1000000);
    microkit_ppcall(2,microkit_msginfo_new(1,1));
}
static void check(nserror error,const char *name){if(error!=NSERROR_OK)native_fail(name,error);}
static struct gui_window *create(struct browser_window *bw,struct gui_window *old,gui_window_create_flags flags){
    (void)old;(void)flags;if(window.bw)return NULL;window.bw=bw;dirty=true;return &window;
}
static void destroy(struct gui_window *w){w->bw=NULL;dirty=true;}
static nserror invalidate(struct gui_window *w,const struct rect *r){(void)w;(void)r;dirty=true;return NSERROR_OK;}
static bool get_scroll(struct gui_window *w,int *x,int *y){*x=0;*y=w->scroll;return true;}
static void scroll_to(int y){
    int max=window.height-(HEIGHT-BAR);if(max<0)max=0;if(y<0)y=0;if(y>max)y=max;
    if(y!=window.scroll){window.scroll=y;dirty=true;printf("NETSURF|SCROLL|%d\n",y);}
}
static nserror set_scroll(struct gui_window *w,const struct rect *r){(void)w;scroll_to(r->y0);return NSERROR_OK;}
static nserror dimensions(struct gui_window *w,int *x,int *y){(void)w;*x=WIDTH;*y=HEIGHT-BAR;return NSERROR_OK;}
static nserror event(struct gui_window *w,enum gui_window_event e){
    if(e==GW_EVENT_UPDATE_EXTENT){browser_window_get_extents(w->bw,true,&w->width,&w->height);scroll_to(w->scroll);}
    if(e==GW_EVENT_REMOVE_CARET)w->caret_h=0;
    if(e==GW_EVENT_START_THROBBER)loaded=false;
    if(e==GW_EVENT_STOP_THROBBER){loaded=true;printf("NETSURF|DOCUMENT|loaded|height=%d\n",w->height);}
    dirty=true;return NSERROR_OK;
}
static void set_title(struct gui_window *w,const char *s){(void)w;(void)s;dirty=true;}
static nserror set_url(struct gui_window *w,struct nsurl *url){
    if(strcmp(address,nsurl_access(url))!=0)w->scroll=0;
    snprintf(address,sizeof(address),"%s",nsurl_access(url));dirty=true;++navigations;
    printf("NETSURF|NAVIGATION|%u\n",navigations);
    if(strstr(address,"result.html?"))printf("NETSURF|FORM|submitted|name=%u|choice=%u\n",strstr(address,"name=nettest")!=NULL,strstr(address,"updates=yes")!=NULL);
    return NSERROR_OK;
}
static void status(struct gui_window *w,const char *s){(void)w;(void)s;}
static void console_log(struct gui_window *w,browser_window_console_source src,const char *msg,size_t len,browser_window_console_flags flags){
    (void)w;(void)src;(void)flags;printf("NETSURF|JS_CONSOLE|%.*s\n",(int)(len>160?160:len),msg);
}
static void pointer(struct gui_window *w,enum gui_pointer_shape s){(void)w;(void)s;}
static void caret(struct gui_window *w,int x,int y,int h,const struct rect *r){(void)r;w->caret_x=x;w->caret_y=y;w->caret_h=h;dirty=true;}
static bool drag(struct gui_window *w,gui_drag_type t,const struct rect *r){(void)w;(void)t;(void)r;return true;}
static const char *filetype(const char *p){
    size_t n=strlen(p);if(n>=3 && strcmp(p+n-3,".js")==0)return "application/javascript";
    if(n>=4 && strcmp(p+n-4,".css")==0)return "text/css";
    if(n>=4 && strcmp(p+n-4,".png")==0)return "image/png";
    if((n>=4 && strcmp(p+n-4,".jpg")==0)||(n>=5 && strcmp(p+n-5,".jpeg")==0))return "image/jpeg";
    if(n>=4 && strcmp(p+n-4,".bmp")==0)return "image/bmp";
    if(n>=4 && strcmp(p+n-4,".ico")==0)return "image/x-icon";
    return "text/html";
}
static nserror resource(const char *path,const uint8_t **data,size_t *length){
    const struct asset *a=native_asset(path);if(!a)return NSERROR_NOT_FOUND;
    *data=a->data;*length=a->size;return NSERROR_OK;
}
static nserror release_resource(const uint8_t *p){(void)p;return NSERROR_OK;}
static struct gui_window_table windows={.create=create,.destroy=destroy,.invalidate=invalidate,.get_scroll=get_scroll,
    .set_scroll=set_scroll,.get_dimensions=dimensions,.event=event,.set_title=set_title,.set_url=set_url,
    .set_status=status,.set_pointer=pointer,.place_caret=caret,.drag_start=drag,.console_log=console_log};
static struct gui_misc_table misc={.schedule=schedule};
static struct gui_fetch_table fetch={.filetype=filetype,.get_resource_data=resource,.release_resource_data=release_resource};
static void navigate(const char *s){
    nsurl *url;
    if(nsurl_create(s,&url)!=NSERROR_OK){editing=true;dirty=true;return;}
    if(!window.bw){struct browser_window *bw;check(browser_window_create(BW_CREATE_HISTORY,url,NULL,NULL,&bw),"create window");}
    else {nserror error=browser_window_navigate(window.bw,url,NULL,BW_NAVIGATE_HISTORY,NULL,NULL,NULL);if(error!=NSERROR_OK){printf("NETSURF|NAVIGATION_ERROR|%d\n",error);native_page_failure("Navigation unavailable");editing=true;}}
    nsurl_unref(url);dirty=true;
}
static void stop_page(void){
    if(window.bw)browser_window_stop(window.bw);
#ifdef NATIVE_HTTPS
    native_https_stop();
#endif
    native_page_stopped();loaded=true;editing=false;dirty=true;
    printf("NETSURF|ACTION|stop\n");
}
static void reload_page(void){
    char target[2048];snprintf(target,sizeof(target),"%s",native_page_target());
    if(!window.bw || !target[0])return;
    stop_page();
    struct hlcache_handle *current=browser_window_get_content(window.bw);
    nserror error=NSERROR_INVALID;
    if(current && !strcmp(nsurl_access(hlcache_handle_get_url(current)),target))
        error=browser_window_reload(window.bw,true);
    if(error!=NSERROR_OK)navigate(target);
    printf("NETSURF|ACTION|reload\n");dirty=true;
}
static void draw(void){
    if(!dirty)return;dirty=false;
    slint_frame_header_t *h=(void*)browser_frame;
    uint32_t before=atomic_load_explicit(&h->revision,memory_order_relaxed);
    atomic_store_explicit(&h->revision,before+1,memory_order_release);atomic_thread_fence(memory_order_seq_cst);
    h->width=PIXEL_WIDTH;h->height=PIXEL_HEIGHT;h->stride_bytes=PIXEL_WIDTH*3;h->format=SLINT_FRAME_FORMAT_RGB8;
    struct rect full={0,0,WIDTH,HEIGHT},page={0,BAR,WIDTH,HEIGHT};
    struct redraw_context ctx={.interactive=true,.background_images=true,.plot=&native_plotters};
    native_plotters.clip(&ctx,&full);native_fill(0,0,WIDTH,HEIGHT,0xfaf6f1);
    if(window.bw && browser_window_redraw_ready(window.bw)){
        native_plotters.clip(&ctx,&page);browser_window_redraw(window.bw,0,BAR-window.scroll,&page,&ctx);
        if(window.caret_h>0)native_fill(window.caret_x,window.caret_y+BAR-window.scroll,1,window.caret_h,0x806010);
    }
    native_plotters.clip(&ctx,&full);native_fill(0,0,WIDTH,BAR,0x231707);
#ifdef NATIVE_DESKTOP
    native_text(10,37,14,0xf4f4e5,"Home");native_text(68,37,14,0xd9b485,"Back");
#else
    native_text(26,38,17,0xf4f4e5,"Home");native_text(112,38,17,0xd9b485,"Back");
#endif
    native_fill(ADDRESS_LEFT,12,ADDRESS_RIGHT-ADDRESS_LEFT,36,0x38281b);
    struct rect address_clip={ADDRESS_LEFT,12,ADDRESS_RIGHT,48};native_plotters.clip(&ctx,&address_clip);
    native_text(ADDRESS_LEFT+10,37,15,0xe5d5b0,editing?typed_url:address);native_plotters.clip(&ctx,&full);
    native_fill(RELOAD_LEFT,12,RELOAD_RIGHT-RELOAD_LEFT,36,0x38281b);native_text(RELOAD_LEFT+5,37,14,0xe5d5b0,"Reload");
    native_fill(STOP_LEFT,12,STOP_RIGHT-STOP_LEFT,36,0x38281b);native_text(STOP_LEFT+8,37,14,0xe5d5b0,"Stop");
#ifdef NATIVE_DESKTOP
    struct rect status_clip={8,50,WIDTH-8,BAR};native_plotters.clip(&ctx,&status_clip);
    native_text(12,68,11,native_page_error()?0x7373ff:0xb5e7a0,native_page_status());
#else
    struct rect status_clip={WIDTH-310,4,WIDTH,56};native_plotters.clip(&ctx,&status_clip);
    native_text(WIDTH-304,37,11,native_page_error()?0x7373ff:0xb5e7a0,native_page_status());
#endif
    native_plotters.clip(&ctx,&full);
    atomic_store_explicit(&h->damage_y,0,memory_order_relaxed);
    atomic_store_explicit(&h->damage_height,PIXEL_HEIGHT,memory_order_relaxed);
    atomic_store_explicit(&h->revision,before+2,memory_order_release);
    microkit_notify(0);++frames;
    if(loaded && window.height>0)printf("NETSURF|READY|frame=%u|height=%d\n",frames,window.height);
}
static void pump(void){
#ifdef NATIVE_DISPLAY_RESIZE
    resize_viewport();
#endif
    for(unsigned budget=0;budget<128;++budget){
        uint64_t now=native_ms();int due=-1;
        for(unsigned i=0;i<128;++i)if(tasks[i].live && tasks[i].due<=now){due=i;break;}
        if(due<0)break;
        struct task current=tasks[due];tasks[due].live=false;current.function(current.data);
    }
    draw();arm();
}
static void key(unsigned code,unsigned value){
    if(code==42||code==54){shift=value!=0;return;}if(!value)return;
    if(!editing && code==63){reload_page();return;}
    if(!editing && code==1){stop_page();return;}
#ifdef NATIVE_BROWSER_TESTS
    if(!editing && code==66){static unsigned test;static const unsigned fail[]={1,2,8,24,48,96};native_reflow_test_arm(fail[test++%6]);return;}
    if(!editing && code==67){native_reflow_test_arm(UINT32_MAX);return;}
    if(!editing && code==68){native_dom_test_expire();return;}
#endif
    unsigned character=0;
    if(code>=2&&code<=10)character="123456789"[code-2];else if(code==11)character='0';
    else if(code>=16&&code<=25)character="qwertyuiop"[code-16];
    else if(code>=30&&code<=38)character="asdfghjkl"[code-30];
    else if(code>=44&&code<=50)character="zxcvbnm"[code-44];
    else if(code==57)character=' ';else if(code==51)character=shift?'<':',';else if(code==52)character=shift?'>':'.';else if(code==53)character=shift?'?':'/';else if(code==39)character=shift?':':';';else if(code==12)character=shift?'_':'-';else if(code==13)character=shift?'+':'=';else if(code==40)character=shift?'\"':'\'';else if(code==14)character=NS_KEY_DELETE_LEFT;
    else if(code==28)character=NS_KEY_CR;else if(code==15)character=shift?NS_KEY_SHIFT_TAB:NS_KEY_TAB;
    else if(code==105)character=NS_KEY_LEFT;else if(code==106)character=NS_KEY_RIGHT;
    else if(code==103)character=NS_KEY_UP;else if(code==108)character=NS_KEY_DOWN;
    else if(code==111)character=NS_KEY_DELETE_RIGHT;
    else if(code==102)character=NS_KEY_TEXT_START;else if(code==107)character=NS_KEY_TEXT_END;
    if(shift&&character>='a'&&character<='z')character-=32;
    if(editing){
        size_t n=strlen(typed_url);
        if(code==1){editing=false;dirty=true;return;}
        if(code==28){editing=false;navigate(typed_url);dirty=true;return;}
        if(code==14){if(n)typed_url[n-1]=0;}
        else if(character>=32&&character<127&&n<sizeof(typed_url)-1){typed_url[n]=character;typed_url[n+1]=0;}
        dirty=true;return;
    }
    bool handled=character&&browser_window_key_press(window.bw,character);
    if(!handled){
        if(code==109)scroll_to(window.scroll+HEIGHT-BAR-50);else if(code==104)scroll_to(window.scroll-HEIGHT+BAR+50);
        else if(code==108)scroll_to(window.scroll+50);else if(code==103)scroll_to(window.scroll-50);
        else if(code==102)scroll_to(0);else if(code==107)scroll_to(window.height);
    }
}
static void pointer_button(unsigned value){
    if ((value!=0)==mouse_down) return;
    bool inside=mouse_x>=0 && mouse_x<WIDTH && mouse_y>=0 && mouse_y<HEIGHT;
    if(inside && mouse_y<BAR){
        if(!value){
            if(mouse_x>=RELOAD_LEFT && mouse_x<RELOAD_RIGHT)reload_page();
            else if(mouse_x>=STOP_LEFT && mouse_x<STOP_RIGHT)stop_page();
            else if(mouse_x>=ADDRESS_LEFT && mouse_x<ADDRESS_RIGHT){editing=true;typed_url[0]=0;dirty=true;}
            else if(mouse_x<HOME_RIGHT)navigate(
#ifdef NATIVE_HTTPS
                "resource:web.html"
#else
                "resource:index.html"
#endif
            );
            else if(mouse_x<BACK_RIGHT && browser_window_back_available(window.bw))browser_window_history_back(window.bw,false);
        }
    } else if(inside) {
        editing=false;browser_window_mouse_click(window.bw,value?BROWSER_MOUSE_PRESS_1:BROWSER_MOUSE_CLICK_1,mouse_x,mouse_y-BAR+window.scroll);
    } else if(!value)browser_window_mouse_track(window.bw,BROWSER_MOUSE_HOVER,mouse_x,mouse_y-BAR+window.scroll);
    mouse_down=value!=0;
}
static void pointer_move(void){
    if(mouse_y>=BAR)browser_window_mouse_track(window.bw,mouse_down?BROWSER_MOUSE_HOLDING_1:BROWSER_MOUSE_HOVER,mouse_x,mouse_y-BAR+window.scroll);
}
#ifdef NATIVE_DESKTOP
/* Private, window-local input supplied by the trusted desktop manager. Never
 * map the raw input-driver ring or global pointer/framebuffer into this PD. */
static void input(void){
    uint32_t *q=(void*)input_handoff;
    uint32_t head=__atomic_load_n(&q[2],__ATOMIC_ACQUIRE);
    if(head-input_tail>224){input_tail=head;mouse_down=shift=false;}
    while(input_tail!=head){
        uint32_t r[4];memcpy(r,q+4+(input_tail++%224)*4,sizeof(r));
        unsigned device=r[0]&65535,kind=r[0]>>16;
        if(device==2){mouse_down=shift=false;continue;}
        if(device==INPUT_DEVICE_KEYBOARD && kind==1 && r[2]<=2){key(r[1],r[2]);continue;}
        if(device!=INPUT_DEVICE_TABLET)continue;
        mouse_x=(int16_t)(r[3]&65535)/NATIVE_SCALE;mouse_y=(int16_t)(r[3]>>16)/NATIVE_SCALE;
        if(kind==3)pointer_move();
        else if(kind==1 && r[1]==272 && r[2]<=1)pointer_button(r[2]);
        else if(kind==2 && r[1]==8)scroll_to(window.scroll-(int32_t)r[2]*80);
    }
    __atomic_store_n(&q[3],input_tail,__ATOMIC_RELEASE);
}
#else
static void input(void){
    struct input_event_ring *q=(void*)input_handoff;
    uint32_t head=__atomic_load_n(&q->head,__ATOMIC_ACQUIRE),lost=__atomic_load_n(&q->dropped,__ATOMIC_ACQUIRE);
    if(head-input_tail>INPUT_EVENT_CAPACITY||lost!=dropped){input_tail=head;dropped=lost;mouse_down=shift=false;}
    while(input_tail!=head){
        struct input_event_record e=q->events[input_tail++&INPUT_EVENT_MASK];if(e.reserved)continue;
        if(e.device==INPUT_DEVICE_TABLET){
            if(e.type==3 && e.code<=1 && e.value<=32767){
                if(e.code==0)mouse_x=e.value*(WIDTH-1)/32767;else mouse_y=e.value*(HEIGHT-1)/32767;
                pointer_move();
            }else if(e.type==1 && e.code==272 && e.value<=1)pointer_button(e.value);
            else if(e.type==2 && e.code==8)scroll_to(window.scroll-(int32_t)e.value*80);
        }else if(e.device==INPUT_DEVICE_KEYBOARD && e.type==1 && e.value<=2)key(e.code,e.value);
    }
    __atomic_store_n(&q->tail,input_tail,__ATOMIC_RELEASE);
    volatile uint32_t *p=(void*)ui_pointer;uint32_t rev=p[0];p[0]=rev+1;__atomic_thread_fence(__ATOMIC_SEQ_CST);
    p[1]=mouse_x*NATIVE_SCALE;p[2]=mouse_y*NATIVE_SCALE;p[3]=1;__atomic_thread_fence(__ATOMIC_RELEASE);p[0]=rev+2;
    microkit_notify(0);
}
#endif

void init(void){
    printf("NETSURF|BOOT|native ARM64|scale=%d\n",NATIVE_SCALE);
#ifndef NATIVE_DESKTOP
    *(uint32_t*)ui_focus=1;
#endif
    native_raster_init((unsigned char*)browser_frame+sizeof(slint_frame_header_t));
    static struct netsurf_table table={.misc=&misc,.window=&windows,.fetch=&fetch,.bitmap=&native_bitmaps,.layout=&native_layout};
    check(netsurf_register(&table),"register");
    check(nsoption_init(NULL,&nsoptions,&nsoptions_default),"options");
    nsoption_set_bool(enable_javascript,
#ifdef NATIVE_JAVASCRIPT
        true
#else
        false
#endif
    );nsoption_set_bool(core_select_menu,true);
    nsoption_set_int(memory_cache_size,8*1024*1024);nsoption_set_uint(disc_cache_size,0);
    const struct asset *messages=native_asset("Messages");if(messages)check(messages_add_from_inline(messages->data,messages->size),"messages");
    bitmap_fmt_t format={.layout=BITMAP_LAYOUT_R8G8B8A8,.pma=false};bitmap_set_format(&format);
    browser_set_dpi(96);check(netsurf_init(NULL),"netsurf init");
#ifdef NATIVE_HTTPS
    check(native_https_register(),"https register");
#endif
    #ifdef NATIVE_HTTPS
    navigate("resource:web.html");
#else
    navigate("resource:index.html");
#endif
    pump();
}
void notified(microkit_channel channel){
#ifdef NATIVE_DISPLAY_RESIZE
    resize_viewport();
#endif
    if(channel==1)input();pump();}
