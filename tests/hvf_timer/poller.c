/* Budgeted poller for an isolated HVF experiment; not a production timer. */
#include <microkit.h>
#include <sel4/sel4.h>
#include <sddf/timer/timer_driver.h>
#include "shared.h"
uintptr_t hvf_timer_state;
void init(void) {}
void notified(microkit_channel ch) {
    if (ch != 0) return;
    hvf_timer_shared *shared = (hvf_timer_shared *)hvf_timer_state;
    uintptr_t frequency; __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(frequency));
    for (;;) {
        uint64_t target = atomic_load_explicit(&shared->deadline_ns, memory_order_acquire);
        if (target == UINT64_MAX) return;
        uint64_t ticks; __asm__ volatile("mrs %0, cntpct_el0" : "=r"(ticks));
        if (ticks_to_ns(ticks, frequency) >= target) { microkit_notify(0); return; }
        seL4_Yield();
    }
}
