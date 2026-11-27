/* SPDX-License-Identifier: MIT
 * Shared production blit generation. Tests rasterize these exact clipped blits
 * into alternating CPU targets and compare an independent full-scene oracle. */
#pragma once
typedef bool (*gpu_emit)(unsigned,int,int,int,int,unsigned,unsigned);
static bool gpu_clip_blit(gpu_emit emit,struct gpu_rect clip,unsigned source,int sx,int sy,int dx,int dy,unsigned w,unsigned h) {
    struct gpu_rect r=gpu_intersect(clip,(struct gpu_rect){dx,dy,dx+(int)w,dy+(int)h});
    return gpu_rect_empty(r)||emit(source,sx+r.x-dx,sy+r.y-dy,r.x,r.y,r.right-r.x,r.bottom-r.y);
}
static bool gpu_compose(const struct gpu_stamp *s,struct gpu_rect clip,gpu_emit emit) {
    unsigned scale=DESKTOP_SCALE,dw=562*scale,dh=(s->app_height+44)*scale,aw=560*scale,ah=s->app_height*scale;
    if(!gpu_clip_blit(emit,clip,10,0,0,0,0,s->width,s->height))return false;
    static const unsigned inset[14]={11,8,6,5,4,3,2,2,1,1,1,0,0,0},corner[10]={0,0,0,1,1,2,3,4,6,9};
    unsigned order[DESKTOP_APP_COUNT];shell_order(&s->scene,order);
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i){unsigned id=order[i];const struct shell_window *w=&s->scene.windows[id-1];
        if(!w->visible||!s->revisions[id+DESKTOP_APP_COUNT])continue;
        int x=w->x*(int)scale,y=w->y*(int)scale,dx=x-(int)scale,dy=y-43*(int)scale;
        unsigned edge=0;while(edge<14&&inset[edge])++edge;
        if(!gpu_clip_blit(emit,clip,10+DESKTOP_APP_COUNT+id,0,edge*scale,dx,dy+edge*(int)scale,dw,dh-2*edge*scale))return false;
        for(unsigned r=0;r<edge;){unsigned n=1;while(r+n<edge&&inset[r+n]==inset[r])++n;
            unsigned left=inset[r]*scale,row=dh-(r+n)*scale;
            if(!gpu_clip_blit(emit,clip,10+DESKTOP_APP_COUNT+id,left,r*scale,dx+left,dy+r*(int)scale,dw-2*left,n*scale)||
               !gpu_clip_blit(emit,clip,10+DESKTOP_APP_COUNT+id,left,row,dx+left,dy+row,dw-2*left,n*scale))return false;
            r+=n;}
        if(!s->revisions[id])continue;
        unsigned first=0;while(first<10&&!corner[first])++first;
        if(!gpu_clip_blit(emit,clip,10+id,0,0,x,y,aw,ah-(10-first)*scale))return false;
        for(unsigned r=first;r<10;){unsigned n=1;while(r+n<10&&corner[r+n]==corner[r])++n;
            unsigned left=corner[r]*scale,row=ah-10*scale+r*scale;
            if(!gpu_clip_blit(emit,clip,10+id,left,row,x+left,y+row,aw-2*left,n*scale))return false;
            r+=n;}
    }
    static const unsigned cursor[11]={1,2,3,4,5,6,7,4,3,2,1};
    if(s->cursor&&s->px>=0&&s->py>=0&&s->px<(int)s->width&&s->py<(int)s->height)
        for(unsigned r=0;r<11;++r){unsigned y=s->py+r*4*scale;if(y>=s->height)break;
            unsigned w=cursor[r]*4*scale,h=4*scale;if(s->px+w>s->width)w=s->width-s->px;if(y+h>s->height)h=s->height-y;
            if(!gpu_clip_blit(emit,clip,20,0,0,s->px,y,w,h))return false;}
    return true;
}
