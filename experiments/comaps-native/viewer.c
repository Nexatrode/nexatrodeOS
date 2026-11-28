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
#include "interaction.h"
#include "search.h"
#ifdef MAP_STORAGE
#include "storage_format.h"
#include "regions_generated.h"
#include "spatial.h"
#ifdef MAP_IMPORT
#include "import_format.h"
#define REGION_STEP 40u
#define REGION_NAME_Y 17
#define REGION_FONT 16
#define REGION_INFO_Y 33
static struct map_local_region import_regions[4];
static struct map_local_pack import_preview;
static unsigned import_state;
static void import_request(unsigned),import_metadata(void),import_reply(void);
static bool import_busy(void){return import_state==1||import_state==3||import_state==4;}
#endif
#ifndef REGION_STEP
#define REGION_STEP 48u
#define REGION_NAME_Y 20
#define REGION_FONT 18
#define REGION_INFO_Y 38
#endif
uintptr_t map_store_control,map_store_bytes;
static unsigned loaded_region=MAP_REGION_COUNT,requested_region,storage_state=1,storage_error;
static uint64_t storage_sequence,storage_generation;
static bool region_picker,load_cancelled,spatial_loading,spatial_refresh,load_detail;
static unsigned spatial_queue[MAP_PAGE_COUNT],spatial_count,spatial_next;
static double spatial_view[4],loaded_view[4];
static unsigned loaded_lod;
static struct map_search_entry region_names[MAP_SEARCH_CATALOGUE_MAX];
static unsigned region_name_count;
static bool region_names_ready,search_jump;
static struct {char name[96];float x,y;unsigned geometry;bool valid;} spatial_selection;
static struct map_io_times load_io;
static uint64_t load_started,load_read_us,load_hash_us,load_decode_us,load_bytes,load_finished,load_progress_draw;
static void spatial_begin(bool),spatial_maybe(void),spatial_reply(void),storage_cancel(void);
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
static int mx,my;
static bool held,changed;
static struct map_gesture gesture;
static struct map_labels last_layout;
static unsigned label_features[MAP_MAX_LABELS],selected_feature;
static uint64_t selection_revision;
static char selection_title[100];
static struct map_search search;
#include "private_state.h"
static bool search_open,search_keyboard,search_shift,search_ctrl;
static int search_pressed=-1,search_scroll;
#ifdef MAP_MOBILE
uintptr_t maps_ui_state;
#endif
static void publish_search_state(void){
#ifdef MAP_MOBILE
    uint32_t *p=(void*)maps_ui_state;uint32_t rev=__atomic_load_n(p,__ATOMIC_RELAXED);
    __atomic_store_n(p,rev+1,__ATOMIC_RELEASE);atomic_thread_fence(memory_order_seq_cst);
    p[1]=active&&((search_open&&search_keyboard)||private_keyboard());p[2]=search_open||private_keyboard();p[3]=private_keyboard();
    __atomic_store_n(p,rev+2,__ATOMIC_RELEASE);
#endif
}
static double cx,cy,units,initial_units;
static unsigned zoom_level;
#ifdef MAP_PROFILE
uintptr_t map_observation;
static uint64_t input_events,input_at;
static void profile(uint64_t start,uint64_t *phases,uint64_t rendered,uint64_t notified) {
    _Atomic uint64_t *p=(void*)map_observation;
    uint64_t revision=atomic_load_explicit(p,memory_order_relaxed);
    atomic_store_explicit(p,revision+1,memory_order_release);atomic_thread_fence(memory_order_seq_cst);
    p[1]=0x4e584d4150303031ULL;p[2]=input_events;p[3]=frames;p[4]=active;
    p[5]=(uint64_t)(int64_t)(cx*1e9);p[6]=(uint64_t)(int64_t)(cy*1e9);p[7]=(uint64_t)(units*1e12);p[8]=zoom_level;
    p[9]=H;p[10]=last_layout.count;
#ifdef MAP_STORAGE
    p[11]=loaded_region<MAP_REGION_COUNT?loaded_region+1:0;p[12]=requested_region+1;
    p[13]=storage_state;p[14]=storage_error;p[15]=storage_generation;
#ifdef MAP_IMPORT
    struct map_store_page *ip=(void*)map_store_control;
    p[960]=0x4e584d494d503031ULL;p[961]=import_state;p[962]=ip->import_phase;p[963]=ip->catalog_generation;p[968]=0;
    for(unsigned i=0;i<4;i++){p[964+i]=import_regions[i].version;if(import_regions[i].present)p[968]|=1u<<i;}
    p[969]=import_preview.region;p[970]=import_preview.version;p[971]=ip->progress;
#endif
    p[880]=region_picker;p[881]=spatial_loading;p[882]=spatial_next;p[883]=spatial_count;
    p[884]=map.feature_count;p[885]=map.point_count;p[886]=load_read_us;p[887]=load_hash_us;p[888]=load_decode_us;
    p[889]=load_finished;p[890]=load_bytes;p[891]=loaded_lod;p[892]=load_cancelled;
    p[893]=region_names_ready;p[894]=region_name_count;
    _Static_assert(901*sizeof(uint64_t)+sizeof(load_io)<=8192,"bounded disk timing summary");
    p[900]=0x4e584d494f303031ULL;memcpy((void*)(p+901),&load_io,sizeof(load_io));
#endif
    _Static_assert(464*8+MAP_MAX_LABELS*4*4<=4096,"bounded label rectangles after timing history");
    uint32_t *rect=(void*)(p+464);
    for(unsigned i=0;i<last_layout.count;++i){struct map_label_rect r=last_layout.rect[i];rect[i*4]=r.left;rect[i*4+1]=r.top;rect[i*4+2]=r.right;rect[i*4+3]=r.bottom;}
    _Static_assert((536+MAP_MAX_LABELS*6)*8<=8192,"bounded public selection metadata");
    p[512]=0x4e584d4445543031ULL;p[513]=selected_feature;p[514]=selection_revision;
    p[515]=held;p[516]=gesture.dragging;p[517]=last_layout.count;p[518]=BOTTOM-94;p[519]=active;
    memset((void*)(p+520),0,96);
    if(selected_feature&&selected_feature<=map.feature_count)memcpy((void*)(p+520),map.features[selected_feature-1].name,96);
    for(unsigned i=0;i<last_layout.count;i++){
        struct map_label_rect r=last_layout.rect[i];unsigned n=536+i*6;
        p[n]=label_features[i]+1;p[n+1]=r.left;p[n+2]=r.top;p[n+3]=r.right;p[n+4]=r.bottom;
        p[n+5]=map.features[label_features[i]].geometry;
    }
    _Static_assert(720+MAP_SEARCH_RESULTS*16<=1024,"bounded public search observations");
    p[704]=0x4e584d5345413031ULL;p[705]=search_open;p[706]=search_keyboard;p[707]=search.count;
    p[708]=search.cursor;p[709]=search.offset;p[710]=search.indexed;p[711]=search.more;
    memset((void*)(p+712),0,56);memcpy((void*)(p+712),search.query,sizeof(search.query));
    for(unsigned i=0;i<search.count;i++){
        unsigned n=720+i*16,id=search.result[i];
        p[n]=id+1;p[n+1]=map_search_geometry(&search,&map,id);p[n+2]=search.rank[i];memcpy((void*)(p+n+3),map_search_name(&search,&map,id),96);
    }
#ifdef MAP_PRIVATE
    p[848]=0x4e584d5052495631ULL;p[849]=private_mode;p[850]=mp_opened();p[851]=places.count;p[852]=private_busy;p[853]=private_generation;
    unsigned cleared=1;const uint8_t *pv=(void*)&places;for(unsigned i=0;i<sizeof(places);i++)if(pv[i])cleared=0;
    for(unsigned i=0;i<sizeof(private_password);i++)if(private_password[i]||private_confirm[i])cleared=0;
    p[854]=cleared&&mp_cleared();p[855]=private_viewed;p[856]=private_ready;
#ifdef MAP_SESSION_PRIVATE
    p[857]=private_session_epoch;
#endif
#endif
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
static void selection_clear(void){if(selected_feature){selected_feature=0;selection_revision++;changed=true;}}
static void selection_set(unsigned feature) {
    if(feature>=map.feature_count||!map.features[feature].name[0])return;
    selected_feature=feature+1;selection_revision++;changed=true;
    const char *name=map.features[feature].name;size_t keep=strlen(name);bool ellipsis=false;
    for(;;){
        snprintf(selection_title,sizeof(selection_title),"%.*s%s",(int)keep,name,ellipsis?"…":"");
        struct map_label_rect bounds;
        if(!map_text_bounds(&font,19*SCALE,selection_title,&bounds)||bounds.right-bounds.left<=(W-84)*SCALE||!keep)break;
        keep--;while(keep&&((unsigned char)name[keep]&192)==128)keep--;ellipsis=true;
    }
}
static void select_at(int x,int y) {
    for(unsigned i=0;i<last_layout.count;i++){
        struct map_label_rect r=last_layout.rect[i];
        if(x*SCALE>=r.left&&x*SCALE<r.right&&y*SCALE>=r.top&&y*SCALE<r.bottom){selection_set(label_features[i]);return;}
    }
#ifdef MAP_MOBILE
    /* Exact label hits win. Nearby touch misses get at least a 44-pixel target;
     * overlapping enlarged targets choose the nearest label center. */
    int best=-1;int64_t distance=INT64_MAX;
    for(unsigned i=0;i<last_layout.count;i++){
        struct map_label_rect r=last_layout.rect[i];int px=(r.left+r.right)/2,py=(r.top+r.bottom)/2;
        int width=r.right-r.left,height=r.bottom-r.top;if(width<44*SCALE)width=44*SCALE;if(height<44*SCALE)height=44*SCALE;
        int64_t dx=x*SCALE-px,dy=y*SCALE-py;
        if(dx>=-width/2&&dx<=width/2&&dy>=-height/2&&dy<=height/2&&dx*dx+dy*dy<distance){distance=dx*dx+dy*dy;best=(int)i;}
    }
    if(best>=0){selection_set(label_features[best]);return;}
#endif
    selection_clear();
}
static double sx(double x){return W/2+(x-cx)/units;}
static double sy(double y){return (TOP+BOTTOM)/2-(y-cy)/units;}
static bool visible(struct map_feature *f){return sx(f->maxx)>=0&&sx(f->minx)<W&&sy(f->miny)>=TOP&&sy(f->maxy)<BOTTOM;}
static unsigned style(const struct map_feature *f) {
    unsigned result=0;
    for(unsigned i=0;i<f->type_count;i++) {
        unsigned t=f->types[i];unsigned s=t<sizeof(map_styles)?map_styles[t]:0;
        if(!result||s==1||s==3||s==5)result=s;
    }
    return result;
}
static bool search_eligible(const struct map_feature *f){unsigned s=style(f);return (f->geometry==0&&s==8)||(f->geometry==1&&(s==4||s==5));}
static void search_rebuild(void){
    map_search_build(&search,&map,search_eligible);
#ifdef MAP_STORAGE
    if(loaded_region==2&&region_names_ready){search.catalogue=region_names;search.indexed=region_name_count;}
#endif
}
static unsigned search_rows(void){int rows=((int)BOTTOM-(TOP+82))/48;return rows<1?1:rows>MAP_SEARCH_RESULTS?MAP_SEARCH_RESULTS:(unsigned)rows;}
static void search_clamp(void){
    unsigned rows=search_rows(),max=search.count>rows?search.count-rows:0;if(search.offset>max)search.offset=max;
    if(search.cursor<search.offset)search.cursor=search.offset;
    if(search.cursor>=search.offset+rows)search.cursor=search.offset+rows-1;
}
static void search_close(void){search_open=search_keyboard=false;search_shift=search_ctrl=false;search_pressed=-1;held=false;map_gesture_cancel(&gesture);changed=true;}
static void search_begin(void){
    if(!map.feature_count)return;
    if(!search_open){selection_clear();search.query[0]=0;map_search_run(&search,&map);}
    search_open=search_keyboard=true;held=false;map_gesture_cancel(&gesture);search_pressed=-1;
#ifdef MAP_STORAGE
    region_picker=false;
#endif
    changed=true;
}
static void clipped_text(int x,int y,int size,uint32_t colour,const char *source,int width,bool tail){
    char display[104];size_t start=0,keep=strlen(source);bool ellipsis=false;
    for(;;){
        snprintf(display,sizeof(display),"%s%.*s%s",ellipsis&&tail?"…":"",(int)keep,source+start,ellipsis&&!tail?"…":"");
        struct map_label_rect r;
        if(!map_text_bounds(&font,size*SCALE,display,&r)||r.right-r.left<=width*SCALE||!keep)break;
        if(tail){start++;while(source[start]&&((unsigned char)source[start]&192)==128)start++;keep=strlen(source+start);}
        else {keep--;while(keep&&((unsigned char)source[keep]&192)==128)keep--;}
        ellipsis=true;
    }
    text(x,y,size,colour,display);
}
static void draw_search(void){
    fill(0,TOP,W,BOTTOM-TOP,0x071521);fill(8,TOP+8,W-64,42,0x123143);fill(8,TOP+48,W-64,2,0x34cddd);
    clipped_text(16,TOP+35,17,search.query[0]?0xe9f7fb:0x91b5c5,search.query[0]?search.query:"Place or road name",W-86,true);
    fill(W-48,TOP+8,40,42,0x183c4a);text(W-38,TOP+37,25,0x92e3e0,"×");
    const char *scope="Search this region offline";
#ifdef MAP_STORAGE
    if(loaded_region==2)scope="Search whole region offline";
#endif
    const char *status=!search.query[0]?scope:!search.count?"No matches in this region":search.more?"First 8 matches · choose a result":"Choose a result";
    text(12,TOP+71,13,0x91b5c5,status);search_clamp();
    for(unsigned row=0;row<search_rows()&&row+search.offset<search.count;row++){
        unsigned i=row+search.offset,id=search.result[i];int y=TOP+82+row*48;
        fill(8,y,W-16,44,i==search.cursor?0x183c4a:0x102433);
        clipped_text(18,y+22,18,0xe9f7fb,map_search_name(&search,&map,id),W-44,false);text(18,y+39,12,0x91b5c5,map_search_geometry(&search,&map,id)==0?"Place":"Road");
    }
    if(search.count>search_rows()){
        int track=BOTTOM-(TOP+82)-6,thumb=track*search_rows()/search.count;
        fill(W-5,TOP+82,2,track,0x183c4a);fill(W-5,TOP+82+track*search.offset/search.count,2,thumb,0x34cddd);
    }
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
#ifdef MAP_PRIVATE
    private_viewed=0;
#endif
    search_close();
    selection_clear();held=false;map_gesture_cancel(&gesture);
    last_layout.count=0;
#ifdef MAP_STORAGE
    if(loaded_region<MAP_REGION_COUNT&&loaded_region!=0){cx=0;cy=0;initial_units=fmax((map.maxx-map.minx)/W,(map.maxy-map.miny)/(BOTTOM-TOP))*1.05;}
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
#ifdef MAP_STORAGE
    if(storage_state==2&&storage_error==MAP_STORE_CAPACITY){storage_state=0;storage_error=0;}
#endif
    if(direction>0&&zoom_level<6){units/=1.5;zoom_level++;}
    if(direction<0&&zoom_level>0){units*=1.5;zoom_level--;}
    last_layout.count=0;changed=true;
}
static void draw(void) {
    if(!changed||!active)return;changed=false;uint64_t started=us(),phases[3];
    publish_search_state();
    slint_frame_header_t *h=(void*)map_frame;
    unsigned before=atomic_load_explicit(&h->revision,memory_order_relaxed);
    atomic_store_explicit(&h->revision,before+1,memory_order_release);atomic_thread_fence(memory_order_seq_cst);
    fill(0,TOP,W,BOTTOM-TOP,0xe9e5d8);
    unsigned drawn=0,labels=0;
    if(!search_open
#ifdef MAP_PRIVATE
       &&!private_mode
#endif
    ){
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
    struct map_labels layout={0};
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
            int label_bottom=selected_feature?BOTTOM-98:BOTTOM-4;
            if(!map_label_admit(&layout,f->name,bounds,(struct map_label_rect){4*SCALE,(TOP+4)*SCALE,(W-4)*SCALE,label_bottom*SCALE}))continue;
            label_features[layout.count-1]=i;
            text(x+1,y+1,size,0xffffff,f->name);text(x,y,size,0x334752,f->name);break;
        }
    }
    labels=layout.count;
    last_layout=layout;
    if(selected_feature&&selected_feature<=map.feature_count){
        struct map_feature *f=&map.features[selected_feature-1];struct map_point p=map.points[f->first+f->count/2];
        int x=sx(p.x),y=sy(p.y);
        if(x>=10&&x<W-10&&y>=TOP+10&&y<(int)BOTTOM-104){
            for(int yy=-9;yy<=9;yy++)for(int xx=-9;xx<=9;xx++)if(xx*xx+yy*yy<=81)fill(x+xx,y+yy,1,1,xx*xx+yy*yy>36?0xffffff:0x19bfd1);
        }
        int top=BOTTOM-94;fill(6,top,W-12,88,0x102c3a);fill(6,top,W-12,2,0x34cddd);
        text(18,top+27,19,0xe9f7fb,selection_title);text(W-40,top+29,26,0x92e3e0,"×");
        unsigned s=style(f);text(18,top+49,14,0x91b5c5,s==8?"Place · offline map":s==4||s==5?"Road · offline map":"Offline map feature");
        double lon=map.origin_x+p.x,lat=atan(sinh((map.origin_y+p.y)*3.141592653589793/180))*180/3.141592653589793;
        char coordinates[80];snprintf(coordinates,sizeof(coordinates),"Map point: %.5f, %.5f",lat,lon);text(18,top+72,13,0x91b5c5,coordinates);
    }
    }else{last_layout.count=0;phases[0]=phases[1]=phases[2]=us();
#ifdef MAP_PRIVATE
        if(private_mode)private_draw();else
#endif
        draw_search();}
#ifdef MAP_PRIVATE
    if(!private_mode&&!search_open)private_card();
#endif
    fill(0,0,W,TOP,0x0b202d);
#if defined(MAP_DESKTOP) || defined(MAP_MOBILE)
#ifdef MAP_STORAGE
    text(12,27,20,0xe9f7fb,map_regions[loaded_region<MAP_REGION_COUNT?loaded_region:requested_region].name);
    char load_status[64];snprintf(load_status,sizeof(load_status),"Loading %u/%u · tap region to cancel",spatial_next,spatial_count);
    text(12,51,13,0x91b5c5,storage_state==1?load_status:storage_state==3?"Saving selection…":storage_state==2?"Load failed · tap region":"Offline · regions");
#else
    text(16,27,22,0xe9f7fb,"Gibraltar");text(16,51,14,0x91b5c5,"CoMaps · offline");
#endif
#else
    text(28,35,29,0xe9f7fb,"CoMaps data · Gibraltar");text(29,67,18,0x91b5c5,"Offline native experiment");
#endif
#ifdef MAP_STORAGE
    if(region_picker&&!search_open){
        fill(6,TOP+4,W-12,MAP_REGION_COUNT*REGION_STEP+56,0x0b202d);
#ifdef MAP_IMPORT
        if(import_state){
            fill(8,TOP+8,W-16,220,0x183c4a);
            text(18,TOP+34,20,0xe9f7fb,"Local offline region");
            if(import_state==2){
                text(18,TOP+65,18,0xe9f7fb,import_preview.name);char info[96];snprintf(info,sizeof(info),"Version %llu · %.2f MB",(unsigned long long)import_preview.version,import_preview.length/1e6);text(18,TOP+89,15,0x91b5c5,info);
                text(18,TOP+113,13,0x91b5c5,"Local pack · publisher not verified");
                fill(16,TOP+136,W-32,38,0x245566);text(28,TOP+162,17,0x92e3e0,import_regions[import_preview.region].present?"Install update":"Import region");
            }else{
                text(18,TOP+72,16,import_state==6?0xffa6ad:0x91b5c5,import_state==6?"Pack rejected · current maps retained":import_state==5?"Installed and verified":"Validating and installing…");
                text(18,TOP+105,13,0x91b5c5,"Previous version kept until durable commit");
            }
            if(!import_busy()){fill(16,TOP+180,W-32,34,0x245566);text(28,TOP+204,16,0x92e3e0,"Back to regions");}
        }else{
#endif
        for(unsigned i=0;i<MAP_REGION_COUNT;i++){
            int y=TOP+6+(int)i*REGION_STEP;fill(8,y,W-16,REGION_STEP-4,i==loaded_region?0x245566:0x183c4a);
            text(16,y+REGION_NAME_Y,REGION_FONT,0xe9f7fb,map_regions[i].name);
            char info[80];snprintf(info,sizeof(info),"%s · %.2f MB%s",
#ifdef MAP_IMPORT
                !import_regions[i].present?"Not installed":
#endif
                i==loaded_region?"Current":"Installed",(i==2?map_pages[64].offset+map_pages[64].length:map_regions[i].length)/1e6,i==2?" · chunked":"");text(16,y+REGION_INFO_Y,12,0x91b5c5,info);
        }
#ifndef MAP_IMPORT
        text(16,TOP+MAP_REGION_COUNT*48+25,13,0x91b5c5,"Fixed memory · validated before switching");
        text(16,TOP+MAP_REGION_COUNT*48+45,13,0x91b5c5,"Offline regions · whole-region name search");
        fill(12,TOP+MAP_REGION_COUNT*48+57,W-24,34,0x183c4a);text(24,TOP+MAP_REGION_COUNT*48+80,16,0x92e3e0,storage_state==1?"Cancel loading":"Close region manager");
#endif
#ifdef MAP_IMPORT
        int action_y=TOP+MAP_REGION_COUNT*REGION_STEP+14;
        fill(12,action_y,W/2-16,38,0x183c4a);text(24,action_y+25,16,0x92e3e0,"Close");
        fill(W/2+4,action_y,W/2-16,38,0x245566);text(W/2+12,action_y+25,16,0x92e3e0,"Import local pack");
        }
#endif
    }else if(storage_state==2&&!search_open){
        fill(6,TOP+6,W-12,54,0x472b32);
        text(14,TOP+28,16,0xffa6ad,storage_error==MAP_STORE_CAPACITY?"View too dense · zoom in":"Region unavailable or damaged");
        text(14,TOP+49,13,0xe9f7fb,loaded_region<MAP_REGION_COUNT?"Choose a region to retry. Current map retained.":"Choose another offline region to continue.");
    }
#endif
    int search_x=BUTTON_LEFT-46;fill(search_x,14,42,42,0x183c4a);
    for(int yy=-7;yy<=7;yy++)for(int xx=-7;xx<=7;xx++)if(xx*xx+yy*yy>=36&&xx*xx+yy*yy<=49)fill(search_x+17+xx,30+yy,1,1,0x92e3e0);
    for(int i=0;i<7;i++)fill(search_x+22+i,35+i,2,2,0x92e3e0);
    const char *buttons[]={"−","+","Fit"};
    for(unsigned i=0;i<3;i++){int x=BUTTON_LEFT+i*BUTTON_STEP;fill(x,14,BUTTON_WIDTH,42,0x183c4a);text(x+12,44,23,0x92e3e0,buttons[i]);}
    fill(0,BOTTOM,W,H-BOTTOM,0x0b202d);
#ifdef MAP_MOBILE
    text(12,H-30,12,0x9cbfc9,"© OpenStreetMap contributors");
    text(12,H-12,12,0x9cbfc9,"CoMaps Project");
    char status[80];snprintf(status,sizeof(status),"Zoom %u",zoom_level);text(W-75,H-12,14,0x91d9d7,status);
#elif defined(MAP_DESKTOP)
    text(12,H-13,12,0x9cbfc9,"© OpenStreetMap contributors · CoMaps Project");
    char status[80];snprintf(status,sizeof(status),"Zoom %u",zoom_level);text(W-75,H-13,14,0x91d9d7,status);
#else
    text(22,785,18,0x9cbfc9,"© OpenStreetMap contributors · ODbL · CoMaps Project / Apache-2.0");
    char status[150];snprintf(status,sizeof(status),"Zoom %u · %u features · drag to pan",zoom_level,drawn);text(895,784,16,0x91d9d7,status);
#endif
#ifdef MAP_PRIVATE
    fill(W-175,H-31,78,27,0x183c4a);text(W-166,H-12,14,0x92e3e0,"SAVED");
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
static void search_accept(unsigned i){
    if(i>=search.count)return;unsigned id=search.result[i];
#ifdef MAP_STORAGE
    if(storage_state==1||storage_state==3)return;
    if(search.catalogue){
        const struct map_search_entry e=search.catalogue[id];
        search_close();zoom_level=3;units=initial_units;for(unsigned n=0;n<zoom_level;n++)units/=1.5;
        cx=e.x;cy=e.y-49*units;constrain();last_layout.count=0;
        memcpy(spatial_selection.name,e.name,96);spatial_selection.x=e.x;spatial_selection.y=e.y;spatial_selection.geometry=e.geometry;
        search_jump=true;
        memset(&load_io,0,sizeof(load_io));load_started=us();load_read_us=load_hash_us=load_decode_us=load_bytes=load_finished=0;
        requested_region=2;spatial_begin(true);return;
    }
#endif
    if(id>=map.feature_count)return;
    const struct map_feature *f=&map.features[id];struct map_point p=map.points[f->first+f->count/2];
    search_close();unsigned level=f->geometry==0?2:3;if(zoom_level<level)zoom_level=level;
    units=initial_units;for(unsigned n=0;n<zoom_level;n++)units/=1.5;
    cx=p.x;cy=p.y-49*units;constrain();last_layout.count=0;selection_set(id);changed=true;
}
static void search_character(unsigned cp){
#ifdef MAP_PRIVATE
    if(private_keyboard()){private_character(cp);return;}
#endif
    if(!search_open)return;
    if(cp==10||cp==13){if(search.count)search_accept(search.cursor);else {search_keyboard=false;changed=true;}return;}
    if(map_search_character(&search,cp)){map_search_run(&search,&map);changed=true;}
}
static unsigned key_character(unsigned code,bool shift){
    static const char letters[]="qwertyuiopasdfghjklzxcvbnm";
    unsigned at=code>=16&&code<=25?code-16:code>=30&&code<=38?code-30+10:code>=44&&code<=50?code-44+19:99;
    if(at<26)return (unsigned)letters[at]-(shift?32:0);
    if(code>=2&&code<=11)return (unsigned)(shift?"!@#$%^&*()":"1234567890")[code-2];
    switch(code){case 12:return shift?'_':'-';case 13:return shift?'+':'=';case 39:return shift?':':';';case 40:return shift?'"':'\'';case 51:return shift?'<':',';case 52:return shift?'>':'.';case 53:return shift?'?':'/';case 57:return ' ';default:return 0;}
}
#ifdef MAP_PRIVATE
#include "private.h"
#endif
static void map_key(unsigned code,unsigned value) {
    if(code==42||code==54){search_shift=value!=0;return;}if(code==29||code==97){search_ctrl=value!=0;return;}
#ifdef MAP_PRIVATE
    private_last_activity=us()/1000;if(private_key(code,value))return;
#endif
    if(value==0||value>2)return;
    if(search_ctrl&&code==33){search_begin();return;}
    if(search_open){
        if(code==1){search_close();return;}
        if(search_ctrl){if(code==22){search.query[0]=0;map_search_run(&search,&map);changed=true;}return;}
        if(code==14||code==111){search_character(8);return;}if(code==28){search_character(10);return;}
        if(code==103||code==108){if(search.count){if(code==103&&search.cursor)search.cursor--;else if(code==108&&search.cursor+1<search.count)search.cursor++;unsigned rows=search_rows();if(search.cursor<search.offset)search.offset=search.cursor;if(search.cursor>=search.offset+rows)search.offset=search.cursor-rows+1;changed=true;}return;}
        unsigned cp=key_character(code,search_shift);if(cp)search_character(cp);return;
    }
    if(code==31||code==53){search_begin();return;}
    if(code==1){selection_clear();held=false;map_gesture_cancel(&gesture);}
    else if(code==13||code==78)zoom(1);else if(code==12||code==74)zoom(-1);
    else if(code==35||code==102)reset();
    else if(code==105){cx-=80*units;changed=true;}else if(code==106){cx+=80*units;changed=true;}
    else if(code==103){cy+=80*units;changed=true;}else if(code==108){cy-=80*units;changed=true;}
    constrain();if(code==103||code==105||code==106||code==108)last_layout.count=0;
}
static void motion(void) {
    int dx,dy;if(held&&map_gesture_move(&gesture,mx,my,&dx,&dy)){
#ifdef MAP_PRIVATE
        if(private_mode==1){private_scroll+=dy;while(private_scroll<=-24){if(private_offset+private_rows()<places.count)private_offset++;private_scroll+=24;}while(private_scroll>=24){if(private_offset)private_offset--;private_scroll-=24;}changed=true;return;}
#endif
        if(search_open){search_scroll+=dy;while(search_scroll<=-24){if(search.offset+search_rows()<search.count)search.offset++;search_scroll+=24;}while(search_scroll>=24){if(search.offset)search.offset--;search_scroll-=24;}changed=true;return;}
        cx-=dx*units;cy+=dy*units;constrain();last_layout.count=0;changed=true;
    }
}
static void map_button(unsigned value) {
#ifdef MAP_IMPORT
    if(import_busy())return;
    if(value&&region_picker&&import_state){
        if(import_state==2&&my>=TOP+136&&my<TOP+174){import_request(MAP_IMPORT_READ);return;}
        if(my>=TOP+180&&my<TOP+214){import_state=0;changed=true;}return;
    }
#endif
#ifdef MAP_PRIVATE
    if(private_button(value))return;
#endif
    if(!value){
        motion();bool tap=map_gesture_end(&gesture);held=false;
        if(search_open){if(tap&&search_pressed>=0&&my>=TOP+82&&my<(int)BOTTOM){unsigned row=(my-TOP-82)/48;if(row<search_rows()&&(unsigned)search_pressed==search.offset+row)search_accept((unsigned)search_pressed);}search_pressed=-1;return;}
#ifdef MAP_STORAGE
        if(storage_state==1||storage_state==3)return;
#endif
        if(tap&&my>=TOP&&my<(int)BOTTOM)select_at(mx,my);
        return;
    }
    held=false;map_gesture_cancel(&gesture);
    if(my>=14&&my<=56&&mx>=BUTTON_LEFT-46&&mx<BUTTON_LEFT){search_begin();return;}
#ifdef MAP_STORAGE
    if(my>=0&&my<TOP&&mx<BUTTON_LEFT){search_close();region_picker=!region_picker;changed=true;return;}
    if(region_picker){
#ifdef MAP_IMPORT
        if(my>=TOP+MAP_REGION_COUNT*REGION_STEP+14&&my<TOP+MAP_REGION_COUNT*REGION_STEP+52){
            if(mx>=W/2)import_request(MAP_IMPORT_PROBE);else{region_picker=false;changed=true;}return;
        }
#endif
        if(my>=TOP+MAP_REGION_COUNT*48+57&&my<TOP+MAP_REGION_COUNT*48+91){if(storage_state==1)storage_cancel();region_picker=false;changed=true;return;}
        if(my>=TOP+6&&my<TOP+6+MAP_REGION_COUNT*REGION_STEP&&storage_state!=1&&storage_state!=3){
            unsigned region=(unsigned)(my-TOP-6)/REGION_STEP;
#ifdef MAP_IMPORT
            if(!import_regions[region].present){changed=true;return;}
#endif
            region_picker=false;held=false;
            recovering=false;storage_request(MAP_STORE_LOAD,region);return;
        }
        region_picker=false;changed=true;return;
    }
    if(loaded_region==MAP_REGION_COUNT)return;
    if(storage_state==2&&!search_open&&my>=TOP+6&&my<TOP+60)return;
#endif
    if(search_open&&my>=(int)TOP&&my<(int)BOTTOM){
        search_pressed=-1;
        if(my<TOP+58){if(mx>=W-52)search_close();else{search_keyboard=true;changed=true;}return;}
        if(my>=TOP+82){unsigned row=(my-TOP-82)/48;if(row<search_rows()&&search.offset+row<search.count){search_pressed=(int)(search.offset+row);search_scroll=0;held=true;map_gesture_begin(&gesture,mx,my);}}
        return;
    }
    if(selected_feature&&my>=(int)BOTTOM-94&&my<(int)BOTTOM-6){
        if(mx>=W-56&&my<(int)BOTTOM-50)selection_clear();return;
    }
    if(my>=TOP&&my<(int)BOTTOM){held=true;map_gesture_begin(&gesture,mx,my);}
    else if(my>=14&&my<=56&&mx>=BUTTON_LEFT){if(mx>=BUTTON_LEFT+2*BUTTON_STEP)reset();else if(mx>=BUTTON_LEFT+BUTTON_STEP)zoom(1);else zoom(-1);}
}
#if defined(MAP_DESKTOP) || defined(MAP_MOBILE)
static void set_height(unsigned height) {
    if(map_height==height)return;
    map_height=height;initial_units=.066/(BOTTOM-TOP);
#ifdef MAP_STORAGE
    if(loaded_region!=0)initial_units=fmax((map.maxx-map.minx)/W,(map.maxy-map.miny)/(BOTTOM-TOP))*1.05;
#endif
    double factor=1.;
    for(unsigned i=0;i<zoom_level;i++)factor*=1.5;
    units=initial_units/factor;changed=true;held=false;map_gesture_cancel(&gesture);last_layout.count=0;
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
    if(head-input_tail>224){
#ifdef MAP_PRIVATE
        private_lock();
#endif
        input_tail=head;held=false;active=false;search_keyboard=search_shift=search_ctrl=false;map_gesture_cancel(&gesture);}
    while(input_tail!=head){
        uint32_t e[4];uint32_t *slot=q+4+(input_tail++%224)*4;memcpy(e,slot,sizeof(e));
#ifdef MAP_PRIVATE
        private_wipe(slot,sizeof(e));
#endif
        unsigned device=e[0]&65535,kind=e[0]>>16;
#ifdef MAP_PROFILE
        input_events++;
#endif
#ifdef MAP_MOBILE
        if(device==3){
            if(kind==0){held=false;
#ifdef MAP_PRIVATE
                if(!e[2]){private_lock();bool was=active;active=true;draw();active=was;}
#endif
                active=e[2]!=0;search_keyboard=search_shift=search_ctrl=false;map_gesture_cancel(&gesture);}
            else if(kind==1&&e[2]>=320&&e[2]<=720)set_height(e[2]);
            else if(active&&kind==2){
#ifdef MAP_PRIVATE
                if(private_keyboard())private_lock();
#endif
                search_keyboard=false;changed=true;}
            else if(active&&kind==3){
#ifdef MAP_PRIVATE
                if(private_keyboard())private_clear_input();else
#endif
                {search.query[0]=0;map_search_run(&search,&map);}changed=true;}
            continue;
        }
#else
        if(device==2){held=false;
#ifdef MAP_PRIVATE
            if(!e[2]){private_lock();bool was=active;active=true;draw();active=was;}
#endif
            active=e[2]!=0;search_keyboard=search_shift=search_ctrl=false;map_gesture_cancel(&gesture);continue;}
#endif
        if(!active)continue;
#ifdef MAP_IMPORT
        /* Drain app input during the single in-flight transaction. Parent
         * focus/lock/resize controls above still run; a search/region command
         * must never replace the shared storage request before completion. */
        if(import_busy())continue;
#endif
        if(device==4&&kind==0){search_character(e[1]);continue;}
        if(device==0&&kind==1){map_key(e[1],e[2]);continue;}
        if(device!=1)continue;
        mx=(int16_t)(e[3]&65535)/SCALE;my=(int16_t)(e[3]>>16)/SCALE;
        if(kind==3&&held)motion();
        else if(kind==1&&e[1]==272&&e[2]<=1)map_button(e[2]);
        else if(kind==2&&e[1]==8){
#ifdef MAP_PRIVATE
            if(private_mode){if(private_mode==1){if((int32_t)e[2]<0&&private_offset+private_rows()<places.count)private_offset++;else if((int32_t)e[2]>0&&private_offset)private_offset--;changed=true;}continue;}
#endif
            if(search_open){if((int32_t)e[2]<0&&search.offset+search_rows()<search.count)search.offset++;else if((int32_t)e[2]>0&&search.offset)search.offset--;changed=true;}else zoom((int32_t)e[2]>0?1:-1);}
    }
    __atomic_store_n(q+3,input_tail,__ATOMIC_RELEASE);
#ifdef MAP_STORAGE
    spatial_maybe();
#endif
    draw();publish_search_state();
#ifdef MAP_PROFILE
    profile(0,NULL,0,0);
#endif
}
#else
static void input(void) {
    struct input_event_ring *q=(void*)input_handoff;
    uint32_t head=__atomic_load_n(&q->head,__ATOMIC_ACQUIRE),lost=__atomic_load_n(&q->dropped,__ATOMIC_ACQUIRE);
    if(head-input_tail>INPUT_EVENT_CAPACITY||lost!=dropped){input_tail=head;dropped=lost;held=false;map_gesture_cancel(&gesture);}
    while(input_tail!=head) {
        struct input_event_record e=q->events[input_tail++&INPUT_EVENT_MASK];if(e.reserved)continue;
        if(e.device==INPUT_DEVICE_TABLET) {
            if(e.type==3&&e.code<=1&&e.value<=32767){if(e.code==0)mx=e.value*(W-1)/32767;else my=e.value*(H-1)/32767;}
            if((e.type==0||e.type==3)&&held)motion();
            if(e.type==1&&e.code==272&&e.value<=1)map_button(e.value);
            if(e.type==2&&e.code==8)zoom((int32_t)e.value>0?1:-1);
        } else if(e.device==INPUT_DEVICE_KEYBOARD&&e.type==1&&e.value<=2)map_key(e.code,e.value);
    }
    __atomic_store_n(&q->tail,input_tail,__ATOMIC_RELEASE);publish_pointer();
#ifdef MAP_STORAGE
    spatial_maybe();
#endif
    draw();
}
#endif
void init(void) {
    uint64_t started=us();
#ifndef MAP_STORAGE
    if(!map_load(&map,map_blob,(size_t)(map_blob_end-map_blob))){printf("COMAPS|FAIL|map=%s|record=%u\n",map.error,map.records);__builtin_trap();}
    search_rebuild();
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
#ifdef MAP_PRIVATE
    private_tick();
#endif
    printf("COMAPS|READY|offline|no network\n");
}
#ifdef MAP_STORAGE
static void storage_cancel(void){
    load_cancelled=true;spatial_refresh=false;changed=true;
}
static void spatial_send(void){
    struct map_store_page *p=(void*)map_store_control;p->command=MAP_STORE_PAGE;p->region=2;p->chunk=spatial_queue[spatial_next];p->length=p->progress=0;p->requested_us=us();
    __atomic_store_n(&p->sequence,++storage_sequence,__ATOMIC_RELEASE);microkit_notify(2);changed=true;
}
static void spatial_begin(bool refresh){
    spatial_refresh=refresh;spatial_loading=true;storage_state=1;storage_error=0;load_cancelled=false;load_progress_draw=us();
    spatial_selection.valid=search_jump||(refresh&&selected_feature&&selected_feature<=map.feature_count);
    if(spatial_selection.valid&&!search_jump){const struct map_feature *f=&map.features[selected_feature-1];struct map_point p=map.points[f->first+f->count/2];memcpy(spatial_selection.name,f->name,96);spatial_selection.x=p.x;spatial_selection.y=p.y;spatial_selection.geometry=f->geometry;}
    search_jump=false;
    selection_clear();search_close();search.query[0]=0;map_search_run(&search,&map);held=false;map_gesture_cancel(&gesture);
    memset(candidate,0,sizeof(*candidate));candidate->origin_x=map_spatial_origin[0];candidate->origin_y=map_spatial_origin[1];
    candidate->minx=map_spatial_bounds[0];candidate->miny=map_spatial_bounds[1];candidate->maxx=map_spatial_bounds[2];candidate->maxy=map_spatial_bounds[3];
    spatial_count=spatial_next=0;load_detail=refresh&&zoom_level>=3;
    if(!load_detail){spatial_queue[spatial_count++]=0;}
    else{
        double dx=W*units*.75,dy=(BOTTOM-TOP)*units*.75;
        spatial_view[0]=cx-dx;spatial_view[1]=cy-dy;spatial_view[2]=cx+dx;spatial_view[3]=cy+dy;
        for(unsigned i=1;i<MAP_PAGE_COUNT;i++){
            // Empty cells have no geometry and need no disk/hash round trip.
            if(map_pages[i].length==16)continue;
            const double *b=map_pages[i].bounds;double ox=candidate->origin_x,oy=candidate->origin_y;
            if(!(b[2]<spatial_view[0]+ox||b[3]<spatial_view[1]+oy||b[0]>spatial_view[2]+ox||b[1]>spatial_view[3]+oy))spatial_queue[spatial_count++]=i;
        }
        if(!spatial_count){spatial_queue[spatial_count++]=0;}
    }
    spatial_send();
}
static void spatial_reply(void){
    struct map_store_page *p=(void*)map_store_control;uint64_t start=us();
    if(!spatial_loading||spatial_next>=spatial_count||p->chunk!=spatial_queue[spatial_next]||p->length!=map_pages[p->chunk].length||
       !map_page_append(candidate,(void*)map_store_bytes,p->length,load_detail?spatial_view:NULL)){
        storage_state=2;storage_error=candidate->error?MAP_STORE_CAPACITY:MAP_STORE_CORRUPT;spatial_loading=false;changed=true;return;
    }
    load_decode_us+=us()-start;spatial_next++;
    if(spatial_next<spatial_count){spatial_send();return;}
    struct offline_map *old=current_map;current_map=candidate;candidate=old;loaded_region=2;spatial_loading=false;
    loaded_lod=load_detail?1:0;memcpy(loaded_view,spatial_view,sizeof(loaded_view));search_rebuild();
    if(!spatial_refresh)reset();
    else if(spatial_selection.valid){for(unsigned i=0;i<map.feature_count;i++){struct map_feature *f=&map.features[i];struct map_point p=map.points[f->first+f->count/2];if(f->geometry==spatial_selection.geometry&&!strcmp(f->name,spatial_selection.name)&&p.x==spatial_selection.x&&p.y==spatial_selection.y){selection_set(i);break;}}}
    spatial_selection.valid=false;
    printf("COMAPS|SPATIAL|pages=%u|features=%u|points=%u|read_us=%llu|hash_us=%llu|decode_us=%llu\n",spatial_count,map.feature_count,map.point_count,(unsigned long long)load_read_us,(unsigned long long)load_hash_us,(unsigned long long)load_decode_us);
    if(spatial_refresh||(recovering&&storage_generation)){recovering=false;storage_state=0;load_finished=us()-load_started;changed=true;}
    else{recovering=false;storage_request(MAP_STORE_COMMIT,loaded_region);}
}
static void spatial_maybe(void){
    if(!active||loaded_region!=2||storage_state||held||search_open||region_picker)return;
#ifdef MAP_PRIVATE
    if(private_mode||private_busy)return;
#endif
    double dx=W*units*.5,dy=(BOTTOM-TOP)*units*.5;
    bool detail=zoom_level>=3;
    if(detail==loaded_lod&&(!detail||(cx-dx>=loaded_view[0]&&cy-dy>=loaded_view[1]&&cx+dx<=loaded_view[2]&&cy+dy<=loaded_view[3])))return;
    memset(&load_io,0,sizeof(load_io));load_started=us();load_read_us=load_hash_us=load_decode_us=load_bytes=load_finished=0;requested_region=2;spatial_begin(true);
}
static void storage_request(unsigned command,unsigned region) {
    struct map_store_page *p=(void*)map_store_control;
    if(command==MAP_STORE_LOAD){
        memset(&load_io,0,sizeof(load_io));load_started=us();load_read_us=load_hash_us=load_decode_us=load_bytes=load_finished=0;load_cancelled=false;
        if(region==2){
            if(region_names_ready){requested_region=region;spatial_refresh=false;spatial_begin(false);return;}
            command=MAP_STORE_SEARCH;
        }
    }
    requested_region=region;storage_error=0;storage_state=command==MAP_STORE_COMMIT?3:1;held=false;
    map_gesture_cancel(&gesture);if(command!=MAP_STORE_COMMIT){selection_clear();search_close();search.query[0]=0;map_search_run(&search,&map);}
    p->command=command;p->region=region;p->length=0;p->requested_us=us();
    __atomic_store_n(&p->sequence,++storage_sequence,__ATOMIC_RELEASE);microkit_notify(2);changed=true;
}
#ifdef MAP_IMPORT
#include "import_view.h"
#endif
static void storage_reply(void) {
    struct map_store_page *p=(void*)map_store_control;
    if(__atomic_load_n(&p->complete,__ATOMIC_ACQUIRE)!=storage_sequence)return;
    storage_generation=p->generation;
#ifdef MAP_IMPORT
    if(p->command>=MAP_IMPORT_PROBE&&p->command<=MAP_IMPORT_COMMIT){import_reply();return;}
    if(p->command==MAP_STORE_BOOT)import_metadata();
#endif
    if(load_cancelled){spatial_loading=false;load_cancelled=false;storage_state=loaded_region<MAP_REGION_COUNT?0:2;changed=true;draw();return;}
    if(p->command==MAP_STORE_LOAD||p->command==MAP_STORE_PAGE){
        struct map_io_times *a=&load_io,*b=&p->io;
        a->requests+=b->requests;a->wait_us+=b->wait_us;a->copy_us+=b->copy_us;a->issue_us+=b->issue_us;a->dispatch_us+=b->dispatch_us;a->viewer_us+=us()-p->done_us;a->empty_irqs+=b->empty_irqs;a->ack_us+=b->ack_us;if(b->max_ack_us>a->max_ack_us)a->max_ack_us=b->max_ack_us;
        if(b->max_wait_us>a->max_wait_us){a->max_wait_us=b->max_wait_us;memcpy(a->worst,b->worst,sizeof(a->worst));}
        if(b->max_copy_us>a->max_copy_us)a->max_copy_us=b->max_copy_us;if(b->max_issue_us>a->max_issue_us)a->max_issue_us=b->max_issue_us;
        for(unsigned i=0;i<8;i++)a->bins[i]+=b->bins[i];
        load_read_us+=p->read_us;load_hash_us+=p->hash_us;load_bytes+=p->length;}
    if(p->result||p->region>=MAP_REGION_COUNT){storage_error=p->result?p->result:MAP_STORE_INVALID;storage_state=2;spatial_loading=false;changed=true;}
    else if(p->command==MAP_STORE_BOOT){storage_request(MAP_STORE_LOAD,(unsigned)p->region);}
    else if(p->command==MAP_STORE_SEARCH){
        double bounds[]={map_spatial_bounds[0]-map_spatial_origin[0]-.001,map_spatial_bounds[1]-map_spatial_origin[1]-.001,map_spatial_bounds[2]-map_spatial_origin[0]+.001,map_spatial_bounds[3]-map_spatial_origin[1]+.001};
        if(p->region!=2||p->length!=map_search_pin.length||!map_search_catalogue_read(region_names,&region_name_count,(void*)map_store_bytes,p->length,bounds)){
            storage_state=2;storage_error=MAP_STORE_CORRUPT;changed=true;
        }else{region_names_ready=true;spatial_begin(false);}
    }
    else if(p->command==MAP_STORE_PAGE){spatial_reply();}
    else if(p->command==MAP_STORE_LOAD){
        uint64_t decode_start=us();
        if(p->region!=requested_region||p->length!=map_regions[requested_region].length||p->length>MAP_FILE_LIMIT||!map_load(candidate,(void*)map_store_bytes,p->length)){
            storage_state=2;storage_error=MAP_STORE_CORRUPT;changed=true;
        }else{
            struct offline_map *old=current_map;current_map=candidate;candidate=old;loaded_region=requested_region;search_rebuild();reset();
            printf("COMAPS|LOADED|region=%s|records=%u|points=%u\n",map_regions[loaded_region].name,map.records,map.point_count);
            if(recovering&&storage_generation){recovering=false;storage_state=0;changed=true;}
            else {recovering=false;storage_request(MAP_STORE_COMMIT,loaded_region);}
        }
        load_decode_us+=us()-decode_start;
    }else if(p->command==MAP_STORE_COMMIT){storage_state=0;changed=true;load_finished=us()-load_started;
#ifdef MAP_PRIVATE
        private_restore();
#endif
    }
    // Chunk completions advance the loader, not the camera. Repainting the
    // retained map for every page dominated larger-region latency. Keep input
    // redraws immediate; cap progress-only redraws at 10 Hz.
    if(spatial_loading&&storage_state==1&&us()-load_progress_draw<100000){
#ifdef MAP_PROFILE
        profile(0,NULL,0,0);
#endif
        return;
    }
    load_progress_draw=us();spatial_maybe();draw();
#ifdef MAP_PROFILE
    profile(0,NULL,0,0);
#endif
}
#endif
void notified(microkit_channel channel){
    if(channel==1)input();
#ifdef MAP_STORAGE
    else if(channel==2){
#ifdef MAP_PRIVATE
        if(private_busy)private_reply();else
#endif
        storage_reply();}
#ifdef MAP_PRIVATE
    else if(channel==4)private_tick();
#endif
#endif
}
