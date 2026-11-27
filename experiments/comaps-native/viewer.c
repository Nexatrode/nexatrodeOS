/* SPDX-License-Identifier: Apache-2.0
 * Diskless CoMaps-data bring-up viewer. All pixels and input run in the guest.
 * No CoMaps Drape renderer, routing, search, location or host map service.
 */
#include <microkit.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <math.h>
#include "mwm.h"
#include "raster.h"
#include "styles.h"
#include "../../tests/input/input_handoff.h"
#include "../../tests/input/slint_frame.h"
#define STB_TRUETYPE_IMPLEMENTATION
#include "stb_truetype.h"
#include "labels.h"
#ifdef MAP_STORAGE
#include "storage_format.h"
#include "regions_generated.h"
uintptr_t map_store_control,map_store_bytes;
static unsigned loaded_region=2,requested_region,storage_state=1,storage_error;
static uint64_t storage_sequence,storage_generation;
static bool region_picker;
static bool recovering=true;
static struct offline_map maps[2],*current_map=&maps[0],*candidate=&maps[1];
#define map (*current_map)
static void storage_request(unsigned,unsigned);
static void storage_reply(void);
#else
static struct offline_map map;
#endif
#ifdef MAP_MOBILE
static unsigned map_height=720;
static bool active;
#define W 360
#define H map_height
#define TOP 64
#define BOTTOM (H-52)
#define BUTTON_LEFT (W-166)
#define BUTTON_STEP 54
#define BUTTON_WIDTH 46
#define SCALE 3
#elif defined(MAP_DESKTOP)
#include "../desktop-gpu/display_protocol.h"
uintptr_t desktop_display_mode;
static unsigned map_height=520;
static bool active;
#define W 560
#define H map_height
#define TOP 64
#define BOTTOM (H-36)
#define BUTTON_LEFT (W-166)
#define BUTTON_STEP 54
#define BUTTON_WIDTH 46
#else
static bool active=true;
#define W 1280
#define H 800
#define TOP 92
#define BOTTOM 756
#define BUTTON_LEFT 1000
#define BUTTON_STEP 85
#define BUTTON_WIDTH 70
#endif
#ifndef SCALE
#define SCALE 2
#endif
uintptr_t map_frame, map_heap, input_handoff, ui_focus, ui_pointer;
extern const unsigned char map_blob[], map_blob_end[], map_font[];
static unsigned char *pixels;
static stbtt_fontinfo font;
static uint32_t input_tail,dropped,frames;
static int mx,my,lastx,lasty;
static bool held,changed;
static double cx,cy,units,initial_units;
static unsigned zoom_level;
#ifdef MAP_PROFILE
uintptr_t map_observation;
static uint64_t input_events,input_at;
static struct map_labels last_layout;
static void profile(uint64_t start,uint64_t *phases,uint64_t rendered,uint64_t notified) {
    _Atomic uint64_t *p=(void*)map_observation;
    uint64_t revision=atomic_load_explicit(p,memory_order_relaxed);
    atomic_store_explicit(p,revision+1,memory_order_release);atomic_thread_fence(memory_order_seq_cst);
    p[1]=0x4e584d4150303031ULL;p[2]=input_events;p[3]=frames;p[4]=active;
    p[5]=(uint64_t)(int64_t)(cx*1e9);p[6]=(uint64_t)(int64_t)(cy*1e9);p[7]=(uint64_t)(units*1e12);p[8]=zoom_level;
    p[9]=H;p[10]=last_layout.count;
#ifdef MAP_STORAGE
    p[11]=loaded_region<2?loaded_region+1:0;p[12]=requested_region+1;
    p[13]=storage_state;p[14]=storage_error;p[15]=storage_generation;
#endif
    _Static_assert(464*8+MAP_MAX_LABELS*4*4<=4096,"bounded label rectangles after timing history");
    uint32_t *rect=(void*)(p+464);
    for(unsigned i=0;i<last_layout.count;++i){struct map_label_rect r=last_layout.rect[i];rect[i*4]=r.left;rect[i*4+1]=r.top;rect[i*4+2]=r.right;rect[i*4+3]=r.bottom;}
    if(start){unsigned slot=frames%32;uint64_t data[14]={frames,input_events,input_at,start,phases[0],phases[1],phases[2],rendered,notified,zoom_level,(uint64_t)(int64_t)(cx*1e9),(uint64_t)(int64_t)(cy*1e9),(uint64_t)(units*1e12),0};
        for(unsigned i=0;i<14;i++)p[16+slot*14+i]=data[i];}
    atomic_store_explicit(p,revision+2,memory_order_release);
}
#endif
static size_t heap_used;
struct glyph { int code,size,w,h,x,y;unsigned char *alpha; };
static struct glyph glyphs[256];static unsigned next_glyph;
void *sbrk(ptrdiff_t amount) {
    if(amount<0||(size_t)amount>16*1024*1024-heap_used){errno=ENOMEM;return (void*)-1;}
    void *p=(void*)(map_heap+heap_used);heap_used+=amount;return p;
}
static int serial_put(char c,FILE *f){(void)f;microkit_dbg_putc(c);return 0;}
static int serial_get(FILE *f){(void)f;return _FDEV_EOF;}
static FILE console=FDEV_SETUP_STREAM(serial_put,serial_get,NULL,_FDEV_SETUP_RW);
FILE *const stdin=&console,*const stdout=&console,*const stderr=&console;
void _exit(int code){printf("COMAPS|FAIL|exit=%d\n",code);__builtin_trap();}
static uint64_t us(void){uint64_t t,f;__asm__ volatile("mrs %0,cntpct_el0":"=r"(t));__asm__ volatile("mrs %0,cntfrq_el0":"=r"(f));return t/f*1000000+(t%f)*1000000/f;}
static void pixel(int x,int y,uint32_t colour,unsigned alpha) {
    if(x<0||y<0||x>=W*SCALE||y>=H*SCALE)return;
    unsigned char *p=pixels+((size_t)y*W*SCALE+x)*3;
    for(unsigned i=0;i<3;i++)p[i]=(p[i]*(255-alpha)+((colour>>(16-8*i))&255)*alpha+127)/255;
}
static void fill(int x,int y,int width,int height,uint32_t colour) {
    map_fill_rgb(pixels,W*SCALE,H*SCALE,x*SCALE,y*SCALE,(x+width)*SCALE,(y+height)*SCALE,colour);
}
static int text(int x,int y,int size,uint32_t colour,const char *s) {
    float pen=x*SCALE,scale=stbtt_ScaleForPixelHeight(&font,size*SCALE);
    while(*s) {
        unsigned cp=map_codepoint(&s);
        struct glyph *g=NULL;
        for(unsigned i=0;i<256;i++)if(glyphs[i].code==(int)cp&&glyphs[i].size==size){g=&glyphs[i];break;}
        if(!g){g=&glyphs[next_glyph++%256];free(g->alpha);g->code=cp;g->size=size;
            g->alpha=stbtt_GetCodepointBitmap(&font,scale,scale,cp,&g->w,&g->h,&g->x,&g->y);}
        if(g->alpha)for(int yy=0;yy<g->h;yy++)for(int xx=0;xx<g->w;xx++)pixel((int)pen+g->x+xx,y*SCALE+g->y+yy,colour,g->alpha[yy*g->w+xx]);
        int advance,bearing;stbtt_GetCodepointHMetrics(&font,cp,&advance,&bearing);pen+=advance*scale;
    }
    return (int)(pen/SCALE)-x;
}
static double sx(double x){return W/2+(x-cx)/units;}
static double sy(double y){return (TOP+BOTTOM)/2-(y-cy)/units;}
static bool visible(struct map_feature *f){return sx(f->maxx)>=0&&sx(f->minx)<W&&sy(f->miny)>=TOP&&sy(f->maxy)<BOTTOM;}
static unsigned style(struct map_feature *f) {
    unsigned result=0;
    for(unsigned i=0;i<f->type_count;i++) {
        unsigned t=f->types[i];unsigned s=t<sizeof(map_styles)?map_styles[t]:0;
        if(!result||s==1||s==3||s==5)result=s;
    }
    return result;
}
/* Clip in double precision before converting large world coordinates to int. */
static void line(double x1,double y1,double x2,double y2,unsigned width,uint32_t colour) {
    double dx=x2-x1,dy=y2-y1,t0=0,t1=1,p[4]={-dx,dx,-dy,dy},q[4]={x1,W-x1,y1-TOP,BOTTOM-1-y1};
    for(unsigned i=0;i<4;i++) {
        if(p[i]==0){if(q[i]<0)return;continue;}
        double t=q[i]/p[i];if(p[i]<0){if(t>t1)return;if(t>t0)t0=t;}else{if(t<t0)return;if(t<t1)t1=t;}
    }
    double ax=(x1+t0*dx)*SCALE,ay=(y1+t0*dy)*SCALE;
    dx=(t1-t0)*dx*SCALE;dy=(t1-t0)*dy*SCALE;
    int steps=(int)fmax(fabs(dx),fabs(dy))+1;
    for(int i=0;i<=steps;i++) {
        int x=(int)(ax+dx*i/steps),y=(int)(ay+dy*i/steps);
        for(int yy=y-(int)width;yy<=y+(int)width;yy++)for(int xx=x-(int)width;xx<=x+(int)width;xx++)
            if(yy>=TOP*SCALE&&yy<BOTTOM*SCALE)pixel(xx,yy,colour,255);
    }
}
static void triangle(struct map_point a,struct map_point b,struct map_point c,uint32_t colour) {
    double x[3]={sx(a.x),sx(b.x),sx(c.x)},y[3]={sy(a.y),sy(b.y),sy(c.y)};
    double min=fmin(y[0],fmin(y[1],y[2])),max=fmax(y[0],fmax(y[1],y[2]));
    int begin=(int)fmax(TOP,ceil(min)),end=(int)fmin(BOTTOM-1,floor(max));
    for(int row=begin;row<=end;row++) {
        double left=W,right=0;unsigned hits=0;
        for(unsigned i=0;i<3;i++){unsigned j=(i+1)%3;
            if((y[i]<=row&&y[j]>row)||(y[j]<=row&&y[i]>row)){
                double at=x[i]+(x[j]-x[i])*(row-y[i])/(y[j]-y[i]);left=fmin(left,at);right=fmax(right,at);hits++;
            }}
        if(hits>=2){int l=(int)fmax(0,ceil(left)),r=(int)fmin(W-1,floor(right));if(r>=l)fill(l,row,r-l+1,1,colour);}
    }
}
static void reset(void) {
#ifdef MAP_STORAGE
    if(loaded_region==1){cx=0;cy=0;initial_units=fmax((map.maxx-map.minx)/W,(map.maxy-map.miny)/(BOTTOM-TOP))*1.05;}
    else
#endif
    {
    cx=-5.346-map.origin_x;
    cy=38.797-map.origin_y;
    initial_units=.066/(BOTTOM-TOP);
    }
    units=initial_units;zoom_level=0;changed=true;
}
static void zoom(int direction) {
    if(direction>0&&zoom_level<6){units/=1.5;zoom_level++;}
    if(direction<0&&zoom_level>0){units*=1.5;zoom_level--;}
    changed=true;
}
static void draw(void) {
    if(!changed||!active)return;changed=false;uint64_t started=us(),phases[3];
    slint_frame_header_t *h=(void*)map_frame;
    unsigned before=atomic_load_explicit(&h->revision,memory_order_relaxed);
    atomic_store_explicit(&h->revision,before+1,memory_order_release);atomic_thread_fence(memory_order_seq_cst);
    fill(0,TOP,W,BOTTOM-TOP,0xe9e5d8);
    unsigned drawn=0;
    for(unsigned pass=0;pass<3;pass++) { for(unsigned i=0;i<map.feature_count;i++) {
        struct map_feature *f=&map.features[i];if(!visible(f))continue;
        unsigned s=style(f);struct map_point *p=map.points+f->first;
        if(f->geometry==2 && pass==0) {
            if(s!=1&&s!=2&&s!=9&&s!=7)continue;
            uint32_t colour=(s==1||s==7)?0x9bcbd6:s==2?0xb6d3ab:0xd4d1b8;
            for(unsigned n=0;n+2<f->count;n+=3)triangle(p[n],p[n+1],p[n+2],colour);drawn++;
        } else if(f->geometry==2&&pass==1&&s==3) {
            for(unsigned n=0;n+2<f->count;n+=3)triangle(p[n],p[n+1],p[n+2],0xb9b3ab);drawn++;
        } else if(f->geometry==1&&pass==2&&s>=4&&s<=7) {
            unsigned width=s==5?3:s==4?1:1;uint32_t colour=s==5?0xd19d62:s==4?0xffffff:s==6?0x657174:0x47889c;
            for(unsigned n=1;n<f->count;n++)line(sx(p[n-1].x),sy(p[n-1].y),sx(p[n].x),sy(p[n].y),width,colour);drawn++;
        }
    }
    phases[pass]=us();}
    /* Places precede streets. Reserve measured glyph/shadow rectangles with
     * padding, and suppress repeated names across segmented road features. */
    struct map_labels layout={0};unsigned labels=0;
    for(unsigned priority=0;priority<2;++priority)for(unsigned i=0;i<map.feature_count&&layout.count<MAP_MAX_LABELS;i++) {
        struct map_feature *f=&map.features[i];unsigned s=style(f);
        if(!f->name[0]||!f->count||!visible(f)||!(priority==0?(f->geometry==0&&s==8):(f->geometry==1&&(s==4||s==5)&&zoom_level>=1)))continue;
        struct map_point p=map.points[f->first+f->count/2];int anchor_x=sx(p.x),anchor_y=sy(p.y);
        if(anchor_x<0||anchor_x>=W||anchor_y<TOP||anchor_y>=BOTTOM)continue;
        int size=s==8?21:15;struct map_label_rect glyph;
        if(!map_text_bounds(&font,size*SCALE,f->name,&glyph))continue;
        int x=anchor_x-(glyph.right+glyph.left)/(2*SCALE);
        for(unsigned attempt=0;attempt<3;++attempt){int y=anchor_y+(attempt==0?-4:attempt==1?size+8:-size-12);
            struct map_label_rect bounds={x*SCALE+glyph.left-3*SCALE,y*SCALE+glyph.top-3*SCALE,x*SCALE+glyph.right+4*SCALE,y*SCALE+glyph.bottom+4*SCALE};
            if(!map_label_admit(&layout,f->name,bounds,(struct map_label_rect){4*SCALE,(TOP+4)*SCALE,(W-4)*SCALE,(BOTTOM-4)*SCALE}))continue;
            text(x+1,y+1,size,0xffffff,f->name);text(x,y,size,0x334752,f->name);break;
        }
    }
    labels=layout.count;
#ifdef MAP_PROFILE
    last_layout=layout;
#endif
    fill(0,0,W,TOP,0x0b202d);
#if defined(MAP_DESKTOP) || defined(MAP_MOBILE)
#ifdef MAP_STORAGE
    text(12,27,20,0xe9f7fb,map_regions[loaded_region<2?loaded_region:requested_region].name);
    text(12,51,13,0x91b5c5,storage_state==1?"Loading…":storage_state==3?"Saving selection…":storage_state==2?"Load failed · tap region":"Offline · change region");
#else
    text(16,27,22,0xe9f7fb,"Gibraltar");text(16,51,14,0x91b5c5,"CoMaps · offline");
#endif
#else
    text(28,35,29,0xe9f7fb,"CoMaps data · Gibraltar");text(29,67,18,0x91b5c5,"Offline native experiment");
#endif
#ifdef MAP_STORAGE
    if(region_picker){
        for(unsigned i=0;i<2;i++){fill(6,TOP+6+(int)i*48,W-12,44,0x183c4a);text(16,TOP+35+(int)i*48,20,0xe9f7fb,map_regions[i].name);}
    }else if(storage_state==2){
        fill(6,TOP+6,W-12,54,0x472b32);
        text(14,TOP+28,16,0xffa6ad,"Region unavailable or damaged");
        text(14,TOP+49,13,0xe9f7fb,loaded_region<2?"Choose a region to retry. Current map retained.":"Choose another offline region to continue.");
    }
#endif
    const char *buttons[]={"−","+","Fit"};
    for(unsigned i=0;i<3;i++){int x=BUTTON_LEFT+i*BUTTON_STEP;fill(x,14,BUTTON_WIDTH,42,0x183c4a);text(x+12,44,23,0x92e3e0,buttons[i]);}
    fill(0,BOTTOM,W,H-BOTTOM,0x0b202d);
#ifdef MAP_MOBILE
    text(12,H-30,12,0x9cbfc9,"© OpenStreetMap contributors");
    text(12,H-12,12,0x9cbfc9,"CoMaps Project");
    char status[80];snprintf(status,sizeof(status),"Zoom %u",zoom_level);text(W-75,H-12,14,0x91d9d7,status);
#elif defined(MAP_DESKTOP)
    text(12,H-13,13,0x9cbfc9,"© OpenStreetMap contributors · CoMaps Project");
    char status[80];snprintf(status,sizeof(status),"Zoom %u",zoom_level);text(W-75,H-13,14,0x91d9d7,status);
#else
    text(22,785,18,0x9cbfc9,"© OpenStreetMap contributors · ODbL · CoMaps Project / Apache-2.0");
    char status[150];snprintf(status,sizeof(status),"Zoom %u · %u features · drag to pan",zoom_level,drawn);text(895,784,16,0x91d9d7,status);
#endif
    h->width=W*SCALE;h->height=H*SCALE;h->stride_bytes=W*SCALE*3;h->format=SLINT_FRAME_FORMAT_RGB8;
    atomic_store_explicit(&h->damage_y,0,memory_order_relaxed);atomic_store_explicit(&h->damage_height,H*SCALE,memory_order_relaxed);
    atomic_store_explicit(&h->revision,before+2,memory_order_release);
    uint64_t rendered=us();microkit_notify(0);uint64_t notified=us();frames++;
#ifdef MAP_PROFILE
    profile(started,phases,rendered,notified);
#else
    (void)phases;(void)rendered;(void)notified;
#endif
    printf("COMAPS|FRAME|n=%u|zoom=%u|cx=%.9f|cy=%.9f|units=%.12f|features=%u|labels=%u|us=%llu\n",frames,zoom_level,cx,cy,units,drawn,labels,(unsigned long long)(us()-started));
}
static void publish_pointer(void) {
#if defined(MAP_DESKTOP) || defined(MAP_MOBILE)
    return; /* The trusted manager owns the global pointer. */
#else
    _Atomic uint32_t *p=(void*)ui_pointer;
    uint32_t revision=atomic_load_explicit(p,memory_order_relaxed);
    atomic_store_explicit(p,revision+1,memory_order_release);
    atomic_thread_fence(memory_order_seq_cst);
    atomic_store_explicit(p+1,mx*SCALE,memory_order_relaxed);
    atomic_store_explicit(p+2,my*SCALE,memory_order_relaxed);
    atomic_store_explicit(p+3,1,memory_order_relaxed);
    atomic_store_explicit(p,revision+2,memory_order_release);
    microkit_notify(0);
#endif
}
static void constrain(void) {
    cx=fmax(map.minx-map.origin_x,fmin(map.maxx-map.origin_x,cx));
    cy=fmax(map.miny-map.origin_y,fmin(map.maxy-map.origin_y,cy));
}
#if defined(MAP_DESKTOP) || defined(MAP_MOBILE)
static void map_key(unsigned code,unsigned value) {
    if(value==0||value>2)return;
    if(code==13||code==78)zoom(1);else if(code==12||code==74)zoom(-1);
    else if(code==35||code==102)reset();
    else if(code==105){cx-=80*units;changed=true;}else if(code==106){cx+=80*units;changed=true;}
    else if(code==103){cy+=80*units;changed=true;}else if(code==108){cy-=80*units;changed=true;}
    constrain();
}
#endif
static void map_button(unsigned value) {
    if(!value){held=false;return;}
#ifdef MAP_STORAGE
    if(my>=0&&my<TOP&&mx<BUTTON_LEFT){region_picker=!region_picker;changed=true;return;}
    if(region_picker){
        if(my>=TOP+6&&my<TOP+102&&storage_state!=1&&storage_state!=3){
            unsigned region=(unsigned)(my-TOP-6)/48;region_picker=false;held=false;
            recovering=false;storage_request(MAP_STORE_LOAD,region);return;
        }
        region_picker=false;changed=true;return;
    }
    if(loaded_region==2)return;
#endif
    if(my>=TOP&&my<(int)BOTTOM){held=true;lastx=mx;lasty=my;}
    else if(my>=14&&my<=56&&mx>=BUTTON_LEFT){if(mx>=BUTTON_LEFT+2*BUTTON_STEP)reset();else if(mx>=BUTTON_LEFT+BUTTON_STEP)zoom(1);else zoom(-1);}
}
#if defined(MAP_DESKTOP) || defined(MAP_MOBILE)
static void set_height(unsigned height) {
    if(map_height==height)return;
    map_height=height;initial_units=.066/(BOTTOM-TOP);
#ifdef MAP_STORAGE
    if(loaded_region==1)initial_units=fmax((map.maxx-map.minx)/W,(map.maxy-map.miny)/(BOTTOM-TOP))*1.05;
#endif
    double factor=1.;
    for(unsigned i=0;i<zoom_level;i++)factor*=1.5;
    units=initial_units/factor;changed=true;held=false;
}
#ifdef MAP_DESKTOP
static void resize(void) {
    const struct display_mode *p=(void*)desktop_display_mode;
    uint32_t first=__atomic_load_n(&p->revision,__ATOMIC_ACQUIRE);struct display_mode mode=*p;
    atomic_thread_fence(memory_order_acquire);
    if(first&&!(first&1)&&first==__atomic_load_n(&p->revision,__ATOMIC_ACQUIRE)&&mode.scale==2&&mode.app_height>=360&&mode.app_height<=520)set_height(mode.app_height);
}
#endif
static void input(void) {
#ifdef MAP_PROFILE
    input_at=us();
#endif
#ifdef MAP_DESKTOP
    resize();
#endif
    uint32_t *q=(void*)input_handoff;uint32_t head=__atomic_load_n(q+2,__ATOMIC_ACQUIRE);
    if(head-input_tail>224){input_tail=head;held=false;active=false;}
    while(input_tail!=head){
        uint32_t e[4];memcpy(e,q+4+(input_tail++%224)*4,sizeof(e));unsigned device=e[0]&65535,kind=e[0]>>16;
#ifdef MAP_PROFILE
        input_events++;
#endif
#ifdef MAP_MOBILE
        if(device==3){
            if(kind==0){held=false;active=e[2]!=0;}
            else if(kind==1&&e[2]>=320&&e[2]<=720)set_height(e[2]);
            continue;
        }
#else
        if(device==2){held=false;active=e[2]!=0;continue;}
#endif
        if(!active)continue;
        if(device==0&&kind==1){map_key(e[1],e[2]);continue;}
        if(device!=1)continue;
        mx=(int16_t)(e[3]&65535)/SCALE;my=(int16_t)(e[3]>>16)/SCALE;
        if(kind==3&&held){cx-=(mx-lastx)*units;cy+=(my-lasty)*units;lastx=mx;lasty=my;constrain();changed=true;}
        else if(kind==1&&e[1]==272&&e[2]<=1)map_button(e[2]);
        else if(kind==2&&e[1]==8)zoom((int32_t)e[2]>0?1:-1);
    }
    __atomic_store_n(q+3,input_tail,__ATOMIC_RELEASE);draw();
#ifdef MAP_PROFILE
    profile(0,NULL,0,0);
#endif
}
#else
static void input(void) {
    struct input_event_ring *q=(void*)input_handoff;
    uint32_t head=__atomic_load_n(&q->head,__ATOMIC_ACQUIRE),lost=__atomic_load_n(&q->dropped,__ATOMIC_ACQUIRE);
    if(head-input_tail>INPUT_EVENT_CAPACITY||lost!=dropped){input_tail=head;dropped=lost;held=false;}
    while(input_tail!=head) {
        struct input_event_record e=q->events[input_tail++&INPUT_EVENT_MASK];if(e.reserved)continue;
        if(e.device==INPUT_DEVICE_TABLET) {
            if(e.type==3&&e.code<=1&&e.value<=32767){if(e.code==0)mx=e.value*(W-1)/32767;else my=e.value*(H-1)/32767;}
            if((e.type==0||e.type==3)&&held){cx-=(mx-lastx)*units;cy+=(my-lasty)*units;lastx=mx;lasty=my;constrain();changed=true;}
            if(e.type==1&&e.code==272&&e.value<=1) {
                if(e.value){
                    if(my>=TOP&&my<BOTTOM){held=true;lastx=mx;lasty=my;}
                    else map_button(1);
                } else held=false;
            }
            if(e.type==2&&e.code==8)zoom((int32_t)e.value>0?1:-1);
        } else if(e.device==INPUT_DEVICE_KEYBOARD&&e.type==1&&e.value==1) {
            if(e.code==13||e.code==78)zoom(1);else if(e.code==12||e.code==74)zoom(-1);
            else if(e.code==35||e.code==102)reset();
            else if(e.code==105){cx-=80*units;changed=true;}else if(e.code==106){cx+=80*units;changed=true;}
            else if(e.code==103){cy+=80*units;changed=true;}else if(e.code==108){cy-=80*units;changed=true;}
            constrain();
        }
    }
    __atomic_store_n(&q->tail,input_tail,__ATOMIC_RELEASE);publish_pointer();draw();
}
#endif
void init(void) {
    uint64_t started=us();
#ifndef MAP_STORAGE
    if(!map_load(&map,map_blob,(size_t)(map_blob_end-map_blob))){printf("COMAPS|FAIL|map=%s|record=%u\n",map.error,map.records);__builtin_trap();}
    printf("COMAPS|MAP|region=Gibraltar|records=%u|points=%u|inner=%u|outer=%u|load_us=%llu\n",map.records,map.point_count,map.inner,map.outer,(unsigned long long)(us()-started));
#else
    (void)started;
#endif
    pixels=(unsigned char*)map_frame+sizeof(slint_frame_header_t);
    if(!stbtt_InitFont(&font,map_font,stbtt_GetFontOffsetForIndex(map_font,0)))__builtin_trap();
    #if !defined(MAP_DESKTOP) && !defined(MAP_MOBILE)
    *(uint32_t*)ui_focus=1;mx=W/2;my=H/2;publish_pointer();
#endif
    reset();
#ifdef MAP_STORAGE
    storage_request(MAP_STORE_BOOT,0);
#endif
    draw();
#ifdef MAP_PROFILE
    profile(0,NULL,0,0);
#endif
    printf("COMAPS|READY|offline|no network\n");
}
#ifdef MAP_STORAGE
static void storage_request(unsigned command,unsigned region) {
    struct map_store_page *p=(void*)map_store_control;
    requested_region=region;storage_error=0;storage_state=command==MAP_STORE_COMMIT?3:1;held=false;
    p->command=command;p->region=region;p->length=0;
    __atomic_store_n(&p->sequence,++storage_sequence,__ATOMIC_RELEASE);microkit_notify(2);changed=true;
}
static void storage_reply(void) {
    struct map_store_page *p=(void*)map_store_control;
    if(__atomic_load_n(&p->complete,__ATOMIC_ACQUIRE)!=storage_sequence)return;
    storage_generation=p->generation;
    if(p->result||p->region>1){storage_error=p->result?p->result:MAP_STORE_INVALID;storage_state=2;changed=true;}
    else if(p->command==MAP_STORE_BOOT){storage_request(MAP_STORE_LOAD,(unsigned)p->region);}
    else if(p->command==MAP_STORE_LOAD){
        if(p->region!=requested_region||p->length!=map_regions[requested_region].length||p->length>MAP_FILE_LIMIT||!map_load(candidate,(void*)map_store_bytes,p->length)){
            storage_state=2;storage_error=MAP_STORE_CORRUPT;changed=true;
        }else{
            struct offline_map *old=current_map;current_map=candidate;candidate=old;loaded_region=requested_region;reset();
            printf("COMAPS|LOADED|region=%s|records=%u|points=%u\n",map_regions[loaded_region].name,map.records,map.point_count);
            if(recovering&&storage_generation){recovering=false;storage_state=0;changed=true;}
            else {recovering=false;storage_request(MAP_STORE_COMMIT,loaded_region);}
        }
    }else if(p->command==MAP_STORE_COMMIT){storage_state=0;changed=true;}
    draw();
#ifdef MAP_PROFILE
    profile(0,NULL,0,0);
#endif
}
#endif
void notified(microkit_channel channel){
    if(channel==1)input();
#ifdef MAP_STORAGE
    else if(channel==2)storage_reply();
#endif
}
