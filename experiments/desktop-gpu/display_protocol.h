/* SPDX-License-Identifier: MIT
 * Native, single-display resize ABI. Each writer owns a separate page.
 */
#pragma once
#include <stdint.h>
#define DISPLAY_DEFAULT_W 2560u
#define DISPLAY_DEFAULT_H 1600u
#define DISPLAY_MAX_W 3840u
#define DISPLAY_MAX_H 2160u
#define DISPLAY_MIN_W 1600u
#define DISPLAY_MIN_H 1200u
struct display_request {uint32_t revision,serial,width,height;};
struct display_mode {uint32_t revision,serial,width,height,scale,app_height;};
struct display_ack {uint32_t revision,serial,width,height;};
static inline uint32_t display_bound(uint32_t n,uint32_t lo,uint32_t hi) {return n<lo?lo:n>hi?hi:n;}
static inline void display_dimensions(uint32_t w,uint32_t h,struct display_mode *m) {
    m->width=display_bound(w,DISPLAY_MIN_W,DISPLAY_MAX_W)&~1u;
    m->height=display_bound(h,DISPLAY_MIN_H,DISPLAY_MAX_H)&~1u;
    m->scale=2;m->app_height=display_bound(m->height/2-220,360,520);
}

/* SPICE single-monitor payload. Byte reads avoid alignment assumptions on the
 * serial stream; reject layouts/depths we do not implement before publication. */
static inline uint32_t display_le32(const uint8_t *p) {return p[0]|((uint32_t)p[1]<<8)|((uint32_t)p[2]<<16)|((uint32_t)p[3]<<24);}
static inline int display_monitor_request(const uint8_t *p,unsigned bytes,uint32_t *w,uint32_t *h) {
    if(bytes!=28||display_le32(p)!=1||display_le32(p+4)>1||display_le32(p+16)!=32||display_le32(p+20)||display_le32(p+24))return 0;
    uint32_t height=display_le32(p+8),width=display_le32(p+12);
    if(!height||!width||height>16384||width>16384)return 0;
    *w=width;*h=height;return 1;
}
