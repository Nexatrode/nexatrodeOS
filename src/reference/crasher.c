#include "protocol.h"

void init(void)
{
    microkit_dbg_puts("CRASHER: ready; no access to server state or service\n");
}

void notified(microkit_channel channel)
{
    if (channel != CRASH_TRIGGER_CHANNEL) {
        microkit_dbg_puts("FAIL: unexpected crasher notification\n");
        return;
    }

    microkit_dbg_puts("CRASHER: attempting forbidden write at 0x40000000\n");
    /* Explicit ARM store guarantees the test executes the intended instruction.
     * This address has no mapping in this PD; it is not a shared-memory pointer.
     */
    __asm__ volatile("str %x[value], [%x[address]]"
                     :
                     : [value] "r" ((seL4_Word)0),
                       [address] "r" (SERVER_PRIVATE_ADDRESS)
                     : "memory");
    microkit_dbg_puts("FAIL: forbidden write unexpectedly succeeded\n");
}
