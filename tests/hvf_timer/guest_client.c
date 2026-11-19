#include <microkit.h>
#include <sddf/timer/client.h>
static uint64_t start;
static unsigned phase;
static uint64_t ticks(void) { uint64_t v; __asm__ volatile("mrs %0, cntpct_el0" : "=r"(v)); return v; }
void init(void) {
    start = ticks();
    sddf_timer_set_timeout(0, 20000000);
    sddf_timer_set_timeout(0, 80000000);
}
void notified(microkit_channel ch) {
    uint64_t frequency; __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(frequency));
    uint64_t elapsed = ticks() - start;
    if (ch != 0 || elapsed >= frequency/2 || (phase == 0 && elapsed < frequency*8/100) || (phase == 1 && elapsed < frequency/50)) {
        microkit_dbg_puts("HVF_TIMER_TEST|FAIL\n"); return;
    }
    if (phase == 0) {
        microkit_dbg_puts("HVF_TIMER_TEST|REPLACE_PASS\n");
        phase = 1; start = ticks(); sddf_timer_set_timeout(0, 20000000);
    } else if (phase == 1) {
        phase = 2; microkit_dbg_puts("HVF_TIMER_TEST|REARM_PASS\n");
    } else {
        microkit_dbg_puts("HVF_TIMER_TEST|FAIL_EXTRA\n");
    }
}
