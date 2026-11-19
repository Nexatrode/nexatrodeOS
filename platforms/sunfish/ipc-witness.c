#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u

static volatile uint32_t *const result = (volatile uint32_t *)0x40000000;

void init(void)
{
    microkit_dbg_puts("SUNFISH IPC WITNESS CLIENT RESULT=");
    microkit_dbg_put32(result[0]);
    microkit_dbg_puts(" SERVICE CALLS=");
    microkit_dbg_put32(result[1]);
    microkit_dbg_puts("\r\nSUNFISH IPC RESET REQUEST\r\n");
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
    microkit_dbg_puts("SUNFISH IPC RESET RETURNED\r\n");
}

void notified(microkit_channel channel) { (void)channel; }
