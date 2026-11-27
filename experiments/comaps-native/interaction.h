/* SPDX-License-Identifier: Apache-2.0 */
#pragma once
#include <stdbool.h>
#include <stdint.h>
struct map_gesture {bool pressed,dragging;int start_x,start_y,last_x,last_y;};
static inline void map_gesture_cancel(struct map_gesture *g){g->pressed=false;g->dragging=false;}
static inline void map_gesture_begin(struct map_gesture *g,int x,int y){*g=(struct map_gesture){true,false,x,y,x,y};}
/* Six logical pixels at either display scale. Jitter never changes the camera;
 * crossing the threshold includes the initial displacement in the first pan. */
static inline bool map_gesture_move(struct map_gesture *g,int x,int y,int *dx,int *dy){
    if(!g->pressed)return false;
    int64_t a=(int64_t)x-g->start_x,b=(int64_t)y-g->start_y;
    if(!g->dragging&&a*a+b*b>36)g->dragging=true;
    if(!g->dragging)return false;
    *dx=x-g->last_x;*dy=y-g->last_y;g->last_x=x;g->last_y=y;
    return *dx||*dy;
}
static inline bool map_gesture_end(struct map_gesture *g){bool tap=g->pressed&&!g->dragging;map_gesture_cancel(g);return tap;}
