/* Compare production damage/blits against an independent per-pixel scene. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#define DESKTOP_APP_COUNT 3
#define DESKTOP_SCALE 1u
#include "../experiments/desktop-shell/protocol.h"
#include "../experiments/desktop-gpu/damage.h"
#include "../experiments/desktop-gpu/composition.h"
static uint32_t image[2][1280*800];
static struct gpu_stamp now,history[2];
static unsigned buffer,blits;
static uint64_t pixels;
static uint32_t color(unsigned source,unsigned x,unsigned y) {
    unsigned rev=source==10?now.revisions[0]:source<=13?now.revisions[source-10]:source<=16?now.revisions[source-10]:0;
    return source==20?0xff40f4ff:0xff000000u|((source*29+rev*17+x*3+y*7)&0xffffff);
}
static bool emit(unsigned source,int sx,int sy,int dx,int dy,unsigned w,unsigned h) {
    assert(sx>=0&&sy>=0&&dx>=0&&dy>=0&&dx+w<=now.width&&dy+h<=now.height);
    assert(source==10||(source>=11&&source<=16)||source==20);
    unsigned sw=source==10?now.width:source<=13?560:source<=16?562:64;
    unsigned sh=source==10?now.height:source<=13?now.app_height:source<=16?now.app_height+44:88;
    assert(sx+w<=sw&&sy+h<=sh);++blits;pixels+=(uint64_t)w*h;
    for(unsigned y=0;y<h;++y)for(unsigned x=0;x<w;++x)image[buffer][(dy+y)*now.width+dx+x]=color(source,sx+x,sy+y);
    return true;
}
static uint32_t reference(unsigned x,unsigned y) {
    uint32_t c=color(10,x,y);unsigned order[3];shell_order(&now.scene,order);
    const unsigned inset[14]={11,8,6,5,4,3,2,2,1,1,1,0,0,0},corner[10]={0,0,0,1,1,2,3,4,6,9};
    for(unsigned i=0;i<3;++i){unsigned id=order[i];struct shell_window *w=&now.scene.windows[id-1];if(!w->visible||!now.revisions[id+3])continue;
        int dx=(int)x-w->x+1,dy=(int)y-w->y+43;unsigned dh=now.app_height+44;
        if(dx>=0&&dy>=0&&dx<562&&dy<(int)dh){unsigned edge=dy<14?(unsigned)dy:dy>=(int)dh-14?dh-1-dy:14;
            unsigned left=edge<14?inset[edge]:0;if((unsigned)dx>=left&&(unsigned)dx<562-left)c=color(13+id,dx,dy);}
        if(now.revisions[id]&&(int)x>=w->x&&x<w->x+560&&(int)y>=w->y&&y<w->y+now.app_height){
            unsigned row=y-w->y,left=row>=now.app_height-10?corner[row-(now.app_height-10)]:0;
            if(x-w->x>=left&&x-w->x<560-left)c=color(10+id,x-w->x,row);}
    }
    const unsigned cursor[11]={1,2,3,4,5,6,7,4,3,2,1};
    if(now.cursor&&now.px>=0&&now.py>=0&&(int)y>=now.py&&(int)y<now.py+44&&(int)x>=now.px&&(int)x<now.px+4*(int)cursor[(y-now.py)/4])c=color(20,0,0);
    return c;
}
static bool matches(bool diagnostic) {
    for(unsigned y=0;y<now.height;++y)for(unsigned x=0;x<now.width;++x)if(image[buffer][y*now.width+x]!=reference(x,y)){if(diagnostic)fprintf(stderr,"mismatch buffer=%u x=%u y=%u actual=%x expected=%x size=%ux%u\n",buffer,x,y,image[buffer][y*now.width+x],reference(x,y),now.width,now.height);return false;}
    return true;
}
static void render(void) {
    now.backdrop_damage=(struct gpu_rect){0,0,now.width,now.height};
    struct gpu_rect damage=gpu_damage(&history[buffer],&now);blits=0;pixels=0;
    assert(gpu_compose(&now,damage,emit)&&blits<=84);assert(matches(true));history[buffer]=now;
}
int main(void) {
    uint32_t bg_history[40]={0};bg_history[5]=2;bg_history[6]=10;bg_history[7]=20;bg_history[8]=30;bg_history[9]=40;
    bg_history[10]=4;bg_history[11]=15;bg_history[12]=25;bg_history[13]=50;bg_history[14]=60;
    struct gpu_rect background=gpu_backdrop_changes(2,4,bg_history,1280,800);assert(background.x==15&&background.bottom==85);
    background=gpu_backdrop_changes(0,4,bg_history,1280,800);assert(background.right==1280&&background.bottom==800);
    background=gpu_backdrop_changes(2,20,bg_history,1280,800);assert(background.right==1280); /* overwritten bg_history */
    bg_history[13]=99999;background=gpu_backdrop_changes(2,4,bg_history,1280,800);assert(background.right==1280); /* malformed */
    now=(struct gpu_stamp){.valid=true,.width=1280,.height=800,.app_height=520,.cursor=true,.px=300,.py=100,.scene={.top=3,.focused=3,.reserved=0x321,.windows={{68,126,1,1},{650,110,1,1},{340,134,1,1}}}};
    for(unsigned i=0;i<7;++i)now.revisions[i]=2;
    buffer=0;render();buffer=1;render();
    /* A cursor-only update damages a small area in each alternating target. */
    now.px+=10;buffer=0;struct gpu_rect r=gpu_damage(&history[0],&now);assert((r.right-r.x)*(r.bottom-r.y)<2000);render();
    now.px+=10;buffer=1;render();
    /* Deliberately omit old bounds: the oracle must detect stale trails. */
    buffer=0;now.scene.windows[0].x=500;assert(gpu_compose(&now,gpu_window(&now,0),emit));assert(!matches(false));render();
    for(unsigned frame=0;frame<100;++frame){buffer=frame&1;
        switch(frame%10){
        case 0:now.scene.windows[0].x=20+(frame*23)%600;break;
        case 1:now.scene.windows[1].visible^=1;break;
        case 2:shell_raise(&now.scene,frame%3+1);break;
        case 3:++now.revisions[frame%3+1];break;
        case 4:++now.revisions[frame%3+4];break;
        case 5:now.cursor=!now.cursor;break;
        case 6:now.px=frame*17%1280;now.py=frame*29%800;break;
        case 7:++now.revisions[0];break;
        case 8:now.width=1000;now.height=760;now.app_height=400;for(unsigned i=0;i<3;++i)if(now.scene.windows[i].x>438)now.scene.windows[i].x=438;break;
        case 9:now.width=1280;now.height=800;now.app_height=520;break;
        }
        render();
    }
    /* Negative control: the other target's history cannot stand in for ours. */
    buffer=0;now.scene.windows[0].visible=1;now.scene.windows[0].x=30;shell_raise(&now.scene,1);render();
    buffer=1;now.scene.windows[0].x=600;render();
    buffer=0;assert(gpu_rect_empty(gpu_damage(&history[1],&now)));assert(!matches(false));render();
    puts("PASS alternating-buffer damage, clipped production blits, cursor trails, stacking, content/focus changes, hide/restore, resize and two stale-pixel negative controls");
}
