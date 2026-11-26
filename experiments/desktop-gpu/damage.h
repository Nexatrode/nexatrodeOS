/* SPDX-License-Identifier: MIT
 * Conservative damage relative to each scanout buffer's last completed frame.
 * One bounded rectangle avoids unbounded command growth under input storms. */
#pragma once
struct gpu_rect {int x,y,right,bottom;};
#define GPU_BACKDROP_HISTORY 8u
#define GPU_BACKDROP_HEADER_BYTES (16u+GPU_BACKDROP_HISTORY*20u)
struct gpu_stamp {
    bool valid,cursor;
    unsigned width,height,app_height;
    int px,py;
    struct shell_state scene;
    uint32_t revisions[7];
    struct gpu_rect backdrop_damage;
};
static inline bool gpu_rect_empty(struct gpu_rect r) {return r.x>=r.right||r.y>=r.bottom;}
static inline struct gpu_rect gpu_intersect(struct gpu_rect a,struct gpu_rect b) {
    return (struct gpu_rect){a.x>b.x?a.x:b.x,a.y>b.y?a.y:b.y,
        a.right<b.right?a.right:b.right,a.bottom<b.bottom?a.bottom:b.bottom};
}
static inline void gpu_union(struct gpu_rect *a,struct gpu_rect b) {
    if(gpu_rect_empty(b))return;
    if(gpu_rect_empty(*a)){*a=b;return;}
    if(b.x<a->x)a->x=b.x;if(b.y<a->y)a->y=b.y;
    if(b.right>a->right)a->right=b.right;if(b.bottom>a->bottom)a->bottom=b.bottom;
}
static inline struct gpu_rect gpu_window(const struct gpu_stamp *s,unsigned i) {
    const struct shell_window *w=&s->scene.windows[i];
    if(!w->visible)return (struct gpu_rect){0};
    return (struct gpu_rect){(w->x-1)*(int)DESKTOP_SCALE,(w->y-43)*(int)DESKTOP_SCALE,
        (w->x+561)*(int)DESKTOP_SCALE,(w->y+(int)s->app_height+1)*(int)DESKTOP_SCALE};
}
static inline struct gpu_rect gpu_cursor(const struct gpu_stamp *s) {
    return s->cursor?(struct gpu_rect){s->px,s->py,s->px+28*(int)DESKTOP_SCALE,s->py+44*(int)DESKTOP_SCALE}:(struct gpu_rect){0};
}
/* Producer history handles multiple dock/clock updates between GPU snapshots.
 * Missing/overwritten generations fall back to a full upload and redraw. */
static inline struct gpu_rect gpu_backdrop_changes(uint32_t previous,uint32_t next,const uint32_t *history,unsigned width,unsigned height) {
    struct gpu_rect full={0,0,width,height},r={0};uint32_t delta=next-previous;
    if(!previous||(delta&1)||delta/2>GPU_BACKDROP_HISTORY)return full;
    for(unsigned n=1;n<=delta/2;++n){uint32_t rev=previous+2*n;const uint32_t *v=history+((rev/2)%GPU_BACKDROP_HISTORY)*5;
        if(v[0]!=rev||v[1]>width||v[2]>height||v[3]>width-v[1]||v[4]>height-v[2])return full;
        gpu_union(&r,(struct gpu_rect){v[1],v[2],v[1]+v[3],v[2]+v[4]});
    }
    return r;
}
static inline struct gpu_rect gpu_damage(const struct gpu_stamp *old,const struct gpu_stamp *next) {
    struct gpu_rect full={0,0,next->width,next->height},r={0};
    if(!old->valid||old->width!=next->width||old->height!=next->height||old->app_height!=next->app_height)return full;
    if(old->revisions[0]!=next->revisions[0])gpu_union(&r,next->backdrop_damage);
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i){
        const struct shell_window *a=&old->scene.windows[i],*b=&next->scene.windows[i];
        if(a->x!=b->x||a->y!=b->y||a->visible!=b->visible){gpu_union(&r,gpu_window(old,i));gpu_union(&r,gpu_window(next,i));}
        if(old->revisions[i+1]!=next->revisions[i+1]||old->revisions[i+4]!=next->revisions[i+4])gpu_union(&r,gpu_window(next,i));
    }
    unsigned before[DESKTOP_APP_COUNT],after[DESKTOP_APP_COUNT],bp[DESKTOP_APP_COUNT],ap[DESKTOP_APP_COUNT];
    shell_order(&old->scene,before);shell_order(&next->scene,after);
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i){bp[before[i]-1]=i;ap[after[i]-1]=i;}
    /* A stacking change affects the overlap, not every window on the desktop. */
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i)for(unsigned j=i+1;j<DESKTOP_APP_COUNT;++j)
        if((bp[i]<bp[j])!=(ap[i]<ap[j])){
            gpu_union(&r,gpu_intersect(gpu_window(old,i),gpu_window(old,j)));
            gpu_union(&r,gpu_intersect(gpu_window(next,i),gpu_window(next,j)));
        }
    if(old->cursor!=next->cursor||old->px!=next->px||old->py!=next->py){gpu_union(&r,gpu_cursor(old));gpu_union(&r,gpu_cursor(next));}
    return gpu_intersect(r,full);
}
