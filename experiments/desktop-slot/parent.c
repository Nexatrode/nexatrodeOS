/* A predeclared slot experiment, not a dynamic process manager. */
#include <microkit.h>
#include <stdint.h>

uintptr_t slot_shared;
static unsigned stage;

void init(void) { stage = 0; }

void notified(microkit_channel channel)
{
    volatile uint64_t *progress = (volatile uint64_t *)slot_shared;
    if (channel != 0) { microkit_dbg_puts("SLOT|FAIL|unexpected channel\n"); return; }
    if (stage == 0) {
#ifndef SLOT_NEGATIVE_NO_STOP
        microkit_pd_stop(1);
#endif
        microkit_notify(0);
        /* The child has higher priority: if it is runnable it can preempt us. */
        if (*progress != 0) { microkit_dbg_puts("SLOT|FAIL|ran while stopped\n"); return; }
        stage = 1;
        microkit_pd_resume(1);
    } else if (stage == 1) {
        if (*progress != 1) { microkit_dbg_puts("SLOT|FAIL|resume\n"); return; }
        microkit_dbg_puts("SLOT|PASS|stop-resume\n");
        stage = 2;
        microkit_notify(0); /* Request the forbidden read. */
    } else {
        microkit_dbg_puts("SLOT|FAIL|unexpected reply\n");
    }
}

seL4_Bool fault(microkit_child child, microkit_msginfo message, microkit_msginfo *reply)
{
    (void)reply;
    if (stage == 2 && child == 1 && microkit_msginfo_get_label(message) == seL4_Fault_VMFault &&
        microkit_msginfo_get_count(message) == seL4_VMFault_Length &&
        microkit_mr_get(seL4_VMFault_Addr) == UINT64_C(0x60000000)) {
        microkit_dbg_puts("SLOT|PASS|private-memory-fault\n");
    } else {
        microkit_dbg_puts("SLOT|FAIL|unexpected fault\n");
    }
    return seL4_False; /* Leave the faulting child stopped. */
}
