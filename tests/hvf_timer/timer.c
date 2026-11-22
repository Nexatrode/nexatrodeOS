/* Experimental HVF timer: only used in isolated test images. */
#include <stdint.h>
#include <os/sddf.h>
#include <sddf/timer/protocol.h>
#include <sddf/timer/config.h>
#include <sddf/timer/timer_driver.h>
#include <sddf/util/udivmodti4.h>
#include "scheduler.h"
#include "shared.h"

uintptr_t hvf_timer_state;
static hvf_timer_queue queue;
static uint64_t frequency;
static hvf_timer_shared *shared(void) { return (hvf_timer_shared *)hvf_timer_state; }
static uint64_t now_ns(void) { uint64_t ticks; __asm__ volatile("mrs %0, cntpct_el0" : "=r"(ticks)); return ticks_to_ns(ticks, frequency); }
static void schedule(void) {
    atomic_store_explicit(&shared()->deadline_ns, hvf_timer_next(&queue), memory_order_release);
    if (hvf_timer_next(&queue) != UINT64_MAX) sddf_notify(0);
}
static void deliver(uint64_t now) {
    uint64_t due = hvf_timer_due(&queue, now);
    for (unsigned i = 1; i < HVF_TIMER_CLIENTS; ++i) if (due & (UINT64_C(1) << i)) sddf_notify(i);
    schedule();
}
void init(void) {
    uintptr_t freq; __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(freq)); frequency = freq;
    hvf_timer_init(&queue);
    atomic_store_explicit(&shared()->deadline_ns, UINT64_MAX, memory_order_release);
}
void notified(sddf_channel ch) { if (ch == 0) deliver(now_ns()); }
seL4_MessageInfo_t protected(sddf_channel ch, seL4_MessageInfo_t msg) {
    if (seL4_MessageInfo_get_label(msg) == SDDF_TIMER_GET_TIME) {
        sddf_set_mr(0, now_ns()); return seL4_MessageInfo_new(0, 0, 0, 1);
    }
    if (seL4_MessageInfo_get_label(msg) == SDDF_TIMER_SET_TIMEOUT && ch > 0 && ch < HVF_TIMER_CLIENTS) {
        uint64_t now = now_ns(); hvf_timer_set(&queue, ch, now, (uint64_t)sddf_get_mr(0)); deliver(now);
    }
    return seL4_MessageInfo_new(0, 0, 0, 0);
}
