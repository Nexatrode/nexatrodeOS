/* SPDX-License-Identifier: Apache-2.0
 * Canonical little-endian native rendering pages. Every page is independently
 * SHA-256 checked by map_store before this structural validation. No allocation.
 */
#pragma once
#include "mwm.h"
#include <math.h>
#include <string.h>
_Static_assert(sizeof(struct map_feature)==140,"spatial-v1 little-endian ABI");
static inline bool map_page_append(struct offline_map *m,const uint8_t *data,size_t length,const double *view){
    if(length<16)return false;uint32_t h[4];memcpy(h,data,16);
    if(h[0]!=0x3150584e||h[1]>MAP_MAX_FEATURES||h[2]>MAP_MAX_POINTS||h[3])return false;
    size_t at=16;unsigned points=0;
    for(unsigned i=0;i<h[1];i++){
        struct map_feature f;if(length-at<sizeof(f))return false;memcpy(&f,data+at,sizeof(f));at+=sizeof(f);
        if(!f.count||f.count>(length-at)/sizeof(struct map_point)||f.geometry>2||!f.type_count||f.type_count>8||!memchr(f.name,0,96))return false;
        if(f.geometry==0&&f.count!=1)return false;if(f.geometry==2&&f.count%3)return false;
        if(!isfinite(f.minx)||!isfinite(f.miny)||!isfinite(f.maxx)||!isfinite(f.maxy)||f.minx>f.maxx||f.miny>f.maxy)return false;
        bool keep=m&&(!view||!(f.maxx<view[0]||f.maxy<view[1]||f.minx>view[2]||f.miny>view[3]));
        if(keep&&(m->feature_count==MAP_MAX_FEATURES||f.count>MAP_MAX_POINTS-m->point_count)){m->error="viewport capacity · zoom in";return false;}
        for(unsigned j=0;j<f.count;j++){
            struct map_point p;memcpy(&p,data+at+j*sizeof(p),sizeof(p));
            if(!isfinite(p.x)||!isfinite(p.y)||p.x<f.minx||p.x>f.maxx||p.y<f.miny||p.y>f.maxy)return false;
        }
        if(keep){f.first=m->point_count;memcpy(m->points+m->point_count,data+at,f.count*sizeof(struct map_point));m->point_count+=f.count;m->features[m->feature_count++]=f;}
        at+=f.count*sizeof(struct map_point);if(f.count>h[2]-points)return false;points+=f.count;
    }
    return at==length&&points==h[2];
}
