/* Bounded CPU hog: a lower-priority witness should run before DONE=1. */
#include <stdint.h>
#include <microkit.h>

static volatile uint32_t *const state = (volatile uint32_t *)0x40000000;

void init(void)
{
    state[0] = 1; /* started */
    state[1] = 0; /* done */
    microkit_dbg_puts("SUNFISH FOUNDATION HOG START\r\n");
    for (volatile uint32_t i = 0; i < 400000000u; ++i) {
        __asm__ volatile("" ::: "memory");
    }
    state[1] = 1;
    microkit_dbg_puts("SUNFISH FOUNDATION HOG DONE\r\n");
}

void notified(microkit_channel channel) { (void)channel; }
