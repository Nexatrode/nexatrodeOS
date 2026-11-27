/* SPDX-License-Identifier: Apache-2.0 */
#include <assert.h>
#include <limits.h>
#include <stdio.h>
#include "../experiments/comaps-native/interaction.h"
int main(void){
    struct map_gesture g={0};int x,y;
    assert(!map_gesture_move(&g,100,100,&x,&y));assert(!map_gesture_end(&g));
    map_gesture_begin(&g,100,200);
    assert(!map_gesture_move(&g,103,202,&x,&y));assert(!map_gesture_move(&g,106,200,&x,&y));
    assert(g.last_x==100&&g.last_y==200);assert(map_gesture_end(&g));
    map_gesture_begin(&g,100,200);assert(map_gesture_move(&g,107,200,&x,&y));assert(x==7&&y==0);
    assert(map_gesture_move(&g,110,204,&x,&y));assert(x==3&&y==4);
    assert(map_gesture_move(&g,100,200,&x,&y));assert(!map_gesture_end(&g)); /* Returning is still a drag. */
    map_gesture_begin(&g,100,200);map_gesture_cancel(&g);assert(!map_gesture_end(&g));
    map_gesture_begin(&g,-32768,-32768);assert(map_gesture_move(&g,32767,32767,&x,&y));assert(!map_gesture_end(&g));
    puts("PASS tap jitter, threshold crossing, complete displacement, held drag, cancellation and coordinate bounds");
}
