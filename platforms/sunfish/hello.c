/* First Pixel 4a seL4/Microkit handoff: UART marker, then PSCI reset. */
#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u

void init(void)
{
    microkit_dbg_puts("\r\nSUNFISH MICROKIT HELLO\r\n");
    /* The UART putchar path waits for each byte to finish transmitting. */
    microkit_dbg_puts("SUNFISH MICROKIT RESET REQUEST\r\n");
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
    microkit_dbg_puts("SUNFISH MICROKIT RESET RETURNED\r\n");
}

void notified(microkit_channel channel)
{
    (void)channel;
}
