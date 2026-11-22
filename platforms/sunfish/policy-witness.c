#include <stdint.h>
#include <microkit.h>
#include "policy-protocol.h"

#define PSCI_SYSTEM_RESET 0x84000009u
static volatile uint32_t *const state = (volatile uint32_t *)0x40000000;

void init(void)
{
    microkit_dbg_puts("SUNFISH POLICY WITNESS VALUE=");
    microkit_dbg_put32(state[0]);
    microkit_dbg_puts(" WRITES=");
    microkit_dbg_put32(state[1]);
    microkit_dbg_puts(" READS=");
    microkit_dbg_put32(state[2]);
    microkit_dbg_puts(" DENIED=");
    microkit_dbg_put32(state[3]);
    microkit_dbg_puts("\r\nSUNFISH POLICY RESET REQUEST\r\n");
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
    microkit_dbg_puts("SUNFISH POLICY RESET RETURNED\r\n");
}

void notified(microkit_channel channel) { (void)channel; }
