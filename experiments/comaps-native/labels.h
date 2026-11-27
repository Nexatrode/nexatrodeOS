/* SPDX-License-Identifier: Apache-2.0 */
#ifndef COMAPS_LABELS_H
#define COMAPS_LABELS_H
#include <stdbool.h>
#include <string.h>
#define MAP_MAX_LABELS 24
struct map_label_rect {int left,top,right,bottom;};
struct map_labels {
    unsigned count;
    struct map_label_rect rect[MAP_MAX_LABELS];
    const char *name[MAP_MAX_LABELS];
};
static unsigned map_codepoint(const char **text) {
    const unsigned char *s=(const unsigned char *)*text;unsigned cp=*s++;
    if(cp>=128){unsigned n=cp<224?1:cp<240?2:3;cp&=(1u<<(6-n))-1;
        while(n--&&*s)cp=(cp<<6)|(*s++&63);}
    *text=(const char *)s;return cp;
}
/* Use the bitmap rasterizer's exact integer pen and glyph bounds. This also
 * covers glyph overhangs, accents and spaces without assuming fixed width. */
static bool map_text_bounds(const stbtt_fontinfo *font,float height,const char *text,struct map_label_rect *r) {
    float scale=stbtt_ScaleForPixelHeight(font,height),pen=0;bool any=false;
    while(*text){unsigned cp=map_codepoint(&text);int l,t,right,b;
        stbtt_GetCodepointBitmapBox(font,cp,scale,scale,&l,&t,&right,&b);
        if(right>l&&b>t){l+=(int)pen;right+=(int)pen;
            if(!any){*r=(struct map_label_rect){l,t,right,b};any=true;}
            else {if(l<r->left)r->left=l;if(t<r->top)r->top=t;if(right>r->right)r->right=right;if(b>r->bottom)r->bottom=b;}}
        int advance,bearing;stbtt_GetCodepointHMetrics(font,cp,&advance,&bearing);pen+=advance*scale;
    }
    return any;
}
static bool map_label_admit(struct map_labels *labels,const char *name,struct map_label_rect r,struct map_label_rect viewport) {
    if(labels->count==MAP_MAX_LABELS||r.left<viewport.left||r.top<viewport.top||r.right>viewport.right||r.bottom>viewport.bottom||r.left>=r.right||r.top>=r.bottom)return false;
    for(unsigned i=0;i<labels->count;++i){struct map_label_rect old=labels->rect[i];
        if(!strcmp(name,labels->name[i])||(r.left<old.right&&old.left<r.right&&r.top<old.bottom&&old.top<r.bottom))return false;}
    labels->rect[labels->count]=r;labels->name[labels->count++]=name;return true;
}
#endif
