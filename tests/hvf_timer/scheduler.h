#pragma once
#include <stdbool.h>
#include <stdint.h>

#define HVF_TIMER_CLIENTS 64

typedef struct { uint64_t deadline[HVF_TIMER_CLIENTS]; } hvf_timer_queue;

static inline void hvf_timer_init(hvf_timer_queue *q) {
    for (unsigned i = 0; i < HVF_TIMER_CLIENTS; ++i) q->deadline[i] = UINT64_MAX;
}

static inline bool hvf_timer_set(hvf_timer_queue *q, unsigned client, uint64_t now, uint64_t delay) {
    if (client >= HVF_TIMER_CLIENTS) return false;
    q->deadline[client] = delay >= UINT64_MAX - now ? UINT64_MAX - 1 : now + delay;
    return true;
}

static inline uint64_t hvf_timer_next(const hvf_timer_queue *q) {
    uint64_t next = UINT64_MAX;
    for (unsigned i = 0; i < HVF_TIMER_CLIENTS; ++i) if (q->deadline[i] < next) next = q->deadline[i];
    return next;
}

static inline uint64_t hvf_timer_due(hvf_timer_queue *q, uint64_t now) {
    uint64_t mask = 0;
    for (unsigned i = 0; i < HVF_TIMER_CLIENTS; ++i) {
        if (q->deadline[i] != UINT64_MAX && q->deadline[i] <= now) {
            mask |= UINT64_C(1) << i;
            q->deadline[i] = UINT64_MAX;
        }
    }
    return mask;
}
