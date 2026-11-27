/* SPDX-License-Identifier: Apache-2.0 */
#ifndef COMAPS_RASTER_H
#define COMAPS_RASTER_H
#include <stdint.h>
#include <string.h>

/* Opaque, clipped RGB8 rectangles. Rasterize one row, then reuse its bytes;
 * map triangles commonly produce two identical physical rows at 2x scale. */
static void map_fill_rgb(uint8_t *pixels,int w,int h,int x,int y,int right,int bottom,uint32_t colour) {
    if(x<0)x=0;if(y<0)y=0;if(right>w)right=w;if(bottom>h)bottom=h;
    if(x>=right||y>=bottom)return;
    uint8_t *row=pixels+((size_t)y*w+x)*3,*p=row;
    uint8_t r=colour>>16,g=colour>>8,b=colour;
    for(int xx=x;xx<right;++xx){*p++=r;*p++=g;*p++=b;}
    size_t bytes=(size_t)(right-x)*3;
    for(int yy=y+1;yy<bottom;++yy)memcpy(row+(size_t)(yy-y)*w*3,row,bytes);
}
#endif
