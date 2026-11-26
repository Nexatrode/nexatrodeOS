/* SPDX-License-Identifier: MIT
 * Software frame cadence; GPU completion is separate from display VSync. */
#pragma once
#include <stdbool.h>
#include <stdint.h>
struct frame_pacer {
    uint64_t period,next_start,pending_since;
    uint32_t coalesced,deferrals;
    bool pending;
};
static inline void frame_pacer_request(struct frame_pacer *p,uint64_t now) {
    if(p->pending)++p->coalesced;
    else {p->pending=true;p->pending_since=now;}
}
static inline bool frame_pacer_ready(const struct frame_pacer *p,uint64_t now,bool busy) {
    return p->pending&&!busy&&now>=p->next_start;
}
static inline uint64_t frame_pacer_take(struct frame_pacer *p,uint64_t now) {
    uint64_t age=now-p->pending_since;p->pending=false;
    /* Rebase after lateness: never submit a catch-up burst. */
    p->next_start=now+p->period;return age;
}
