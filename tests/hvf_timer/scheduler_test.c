#include <assert.h>
#include <stdint.h>
#include "scheduler.h"

int main(void) {
    hvf_timer_queue q;
    hvf_timer_init(&q);
    assert(hvf_timer_next(&q) == UINT64_MAX);
    assert(hvf_timer_set(&q, 1, 100, 50));
    assert(hvf_timer_set(&q, 2, 100, 20));
    assert(hvf_timer_next(&q) == 120);
    assert(hvf_timer_due(&q, 119) == 0);
    assert(hvf_timer_due(&q, 120) == (UINT64_C(1) << 2));
    assert(hvf_timer_next(&q) == 150);
    assert(hvf_timer_set(&q, 1, 130, 100));
    assert(hvf_timer_due(&q, 150) == 0);
    assert(hvf_timer_due(&q, 230) == (UINT64_C(1) << 1));
    assert(hvf_timer_next(&q) == UINT64_MAX);
    assert(!hvf_timer_set(&q, 64, 0, 10));
    assert(hvf_timer_set(&q, 3, UINT64_MAX - 3, 10));
    assert(hvf_timer_next(&q) == UINT64_MAX - 1);
    assert(hvf_timer_due(&q, UINT64_MAX - 1) == (UINT64_C(1) << 3));
    return 0;
}
