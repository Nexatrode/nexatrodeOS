/* SPDX-License-Identifier: BSD-2-Clause
 * Budgeted architectural-counter poller for this isolated accelerated VM.
 */
#include <microkit.h>
#include <sel4/sel4.h>
#include <stdatomic.h>
uintptr_t hvf_timer_state;
void init(void) {}
void notified(microkit_channel channel) {
    if (channel != 0) return;
    uint64_t frequency; __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(frequency));
    for (;;) {
        uint64_t deadline = atomic_load_explicit((_Atomic uint64_t *)hvf_timer_state, memory_order_acquire);
        if (deadline == UINT64_MAX) return;
        uint64_t ticks; __asm__ volatile("mrs %0, cntpct_el0" : "=r"(ticks));
        uint64_t seconds = ticks / frequency;
        uint64_t now = seconds * 1000000000 + (ticks % frequency) * 1000000000 / frequency;
        if (now >= deadline) { microkit_notify(0); return; }
        seL4_Yield();
    }
}
