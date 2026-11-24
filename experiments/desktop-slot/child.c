#include <microkit.h>
#include <stdint.h>

uintptr_t slot_shared;
static unsigned calls;

void init(void) { calls = 0; microkit_notify(0); }

void notified(microkit_channel channel)
{
    if (channel != 0) { microkit_dbg_puts("SLOT|FAIL|child channel\n"); return; }
    if (calls++ == 0) {
        *(volatile uint64_t *)slot_shared = 1;
        microkit_notify(0);
    } else {
        /* Parent-only virtual address, deliberately absent from this PD. */
        (void)*(volatile uint64_t *)UINT64_C(0x60000000);
        microkit_dbg_puts("SLOT|FAIL|private read succeeded\n");
    }
}
