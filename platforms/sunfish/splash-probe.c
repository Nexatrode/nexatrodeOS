/* Read-only check of the bootloader's reserved continuous-splash region. */
#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u
#define SPLASH_BYTES 0x1000000u /* first 16 MiB of 23 MiB reservation */

uintptr_t splash_vaddr;

void init(void)
{
    microkit_dbg_puts("\r\nSUNFISH SPLASH PROBE START\r\n");
    volatile const uint32_t *pixels = (volatile const uint32_t *)splash_vaddr;
    uint32_t nonzero = 0;
    uint32_t mixed = 0;
    for (uint32_t offset = 0; offset < SPLASH_BYTES; offset += 4096) {
        uint32_t word = pixels[offset / sizeof(uint32_t)];
        nonzero += word != 0;
        mixed ^= word + (offset >> 12);
    }
    microkit_dbg_puts("SPLASH SAMPLE PAGES ");
    microkit_dbg_put32(SPLASH_BYTES / 4096);
    microkit_dbg_puts(" NONZERO ");
    microkit_dbg_put32(nonzero);
    microkit_dbg_puts(" MIX ");
    microkit_dbg_put32(mixed);
    microkit_dbg_puts("\r\nSUNFISH SPLASH RESET REQUEST\r\n");

    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
    microkit_dbg_puts("SUNFISH SPLASH RESET RETURNED\r\n");
}

void notified(microkit_channel channel)
{
    (void)channel;
}
