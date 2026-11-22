/* Observe preemption and recovery after another PD faults, then reset. */
#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u
#define WAIT_LIMIT 1600000000u

static volatile uint32_t *const state = (volatile uint32_t *)0x40000000;

static void print_state(const char *label)
{
    microkit_dbg_puts(label);
    microkit_dbg_puts(" STARTED=");
    microkit_dbg_put32(state[0]);
    microkit_dbg_puts(" DONE=");
    microkit_dbg_put32(state[1]);
    microkit_dbg_puts("\r\n");
}

void init(void)
{
    print_state("SUNFISH FOUNDATION WITNESS FIRST");
    for (volatile uint32_t i = 0; i < WAIT_LIMIT && state[1] == 0; ++i) {
        __asm__ volatile("" ::: "memory");
    }
    print_state("SUNFISH FOUNDATION WITNESS FINAL");
    microkit_dbg_puts("SUNFISH FOUNDATION RESET REQUEST\r\n");
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
    microkit_dbg_puts("SUNFISH FOUNDATION RESET RETURNED\r\n");
}

void notified(microkit_channel channel) { (void)channel; }
