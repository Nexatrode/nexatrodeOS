/* Reset the temporary Slint display test after an observation window. */
#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u
#ifndef RESET_OUTER
#define RESET_OUTER 2u
#endif

void init(void)
{
#if RESET_OUTER == 0
    microkit_dbg_puts("SUNFISH SLINT AUTO RESET DISABLED\r\n");
    /* Keep this otherwise-unused PD asleep/yielding while the display stays up. */
    for (;;) {
        seL4_Yield();
    }
#else
    microkit_dbg_puts("SUNFISH SLINT WATCHDOG START\r\n");
    /* Roughly a short observation window on the boot CPU. This is not a
     * calibrated timer; the service has no EL0 counter access yet. */
    for (uint32_t outer = 0; outer < RESET_OUTER; outer++) {
        for (volatile uint32_t inner = 0; inner < 500000000u; inner++) {
            __asm__ volatile("" ::: "memory");
        }
    }
    microkit_dbg_puts("SUNFISH SLINT RESET REQUEST\r\n");
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
    microkit_dbg_puts("SUNFISH SLINT RESET RETURNED\r\n");
#endif
}

void notified(microkit_channel channel) { (void)channel; }
