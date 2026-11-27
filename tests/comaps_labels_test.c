/* Verify font bounds against actual glyph bitmaps, plus collision/dedup rules. */
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#define STB_TRUETYPE_IMPLEMENTATION
#include "stb_truetype.h"
#include "../experiments/comaps-native/labels.h"
int main(int argc,char **argv) {
    assert(argc==2);FILE *f=fopen(argv[1],"rb");assert(f);fseek(f,0,SEEK_END);long n=ftell(f);rewind(f);
    unsigned char *data=malloc(n);assert(data&&fread(data,1,n,f)==(size_t)n);fclose(f);
    stbtt_fontinfo font;assert(stbtt_InitFont(&font,data,stbtt_GetFontOffsetForIndex(data,0)));
    const char *names[]={"Gibraltar","North Mole","Sir Herbert Road","St Michael's Road","Iñaki — Café","  Wide WWW / iii  "};
    for(unsigned scale=2;scale<=3;++scale)for(unsigned i=0;i<sizeof(names)/sizeof(*names);++i){
        struct map_label_rect r;assert(map_text_bounds(&font,21*scale,names[i],&r));
        float factor=stbtt_ScaleForPixelHeight(&font,21*scale),pen=0;const char *s=names[i];
        while(*s){unsigned cp=map_codepoint(&s);int w,h,x,y,advance,bearing;
            unsigned char *bitmap=stbtt_GetCodepointBitmap(&font,factor,factor,cp,&w,&h,&x,&y);
            for(int yy=0;yy<h;++yy)for(int xx=0;xx<w;++xx)if(bitmap[yy*w+xx]){
                assert((int)pen+x+xx>=r.left&&(int)pen+x+xx<r.right&&y+yy>=r.top&&y+yy<r.bottom);
            }
            stbtt_FreeBitmap(bitmap,NULL);stbtt_GetCodepointHMetrics(&font,cp,&advance,&bearing);pen+=advance*factor;
        }
    }
    struct map_labels labels={0};struct map_label_rect view={0,0,1000,1000};
    assert(map_label_admit(&labels,"Gibraltar",(struct map_label_rect){10,10,110,40},view));
    assert(!map_label_admit(&labels,"Gibraltar",(struct map_label_rect){210,210,310,240},view));
    assert(!map_label_admit(&labels,"Road",(struct map_label_rect){109,39,150,60},view));
    assert(!map_label_admit(&labels,"Clipped",(struct map_label_rect){-1,50,90,80},view));
    assert(map_label_admit(&labels,"Road",(struct map_label_rect){110,10,180,40},view));
    char name[MAP_MAX_LABELS][20];
    for(unsigned i=2;i<MAP_MAX_LABELS;++i){snprintf(name[i],sizeof(name[i]),"label%u",i);
        assert(map_label_admit(&labels,name[i],(struct map_label_rect){0,(int)i*10+100,50,(int)i*10+105},view));}
    assert(!map_label_admit(&labels,"Overflow",(struct map_label_rect){700,700,750,720},view));
    free(data);puts("PASS 2x/3x glyph bitmap bounds, duplicate names, collisions, clipping and label capacity");
}
