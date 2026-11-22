/* Temporary, reversible test of the bootloader's reserved splash buffer. */
#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u
#define WIDTH 1080u
#define HEIGHT 2340u
#define FRAME_BYTES (WIDTH * HEIGHT * 4u)

uintptr_t splash_vaddr;

static void reset(void)
{
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
    microkit_dbg_puts("SUNFISH SPLASH PATTERN RESET RETURNED\r\n");
}

void init(void)
{
    volatile uint32_t *frame = (volatile uint32_t *)splash_vaddr;
    microkit_dbg_puts("\r\nSUNFISH SPLASH PATTERN START\r\n");

    /* The 23 MiB reserved region holds at least two 1080x2340x4 frames.
     * Touch no memory outside the two frame-sized ranges. */
    for (uint32_t buffer = 0; buffer < 2; buffer++) {
        volatile uint32_t *pixels = frame + buffer * (FRAME_BYTES / 4u);
        for (uint32_t y = 0; y < HEIGHT; y++) {
            uint32_t colour = ((y / 195u) & 1u) ? 0x00ffffffu : 0x00000000u;
            for (uint32_t x = 0; x < WIDTH; x++) {
                pixels[y * WIDTH + x] = colour;
            }
        }
    }
    __asm__ volatile("dsb sy" ::: "memory");
    microkit_dbg_puts("SUNFISH SPLASH PATTERN WRITTEN\r\n");

    /* Leave a short observation window. cntvct_el0 is not accessible in
     * this PD, so use a bounded instruction loop instead of a timer. */
    for (uint32_t outer = 0; outer < 10; outer++) {
        for (volatile uint32_t inner = 0; inner < 500000000u; inner++) {
            __asm__ volatile("" ::: "memory");
        }
    }
    microkit_dbg_puts("SUNFISH SPLASH PATTERN RESET REQUEST\r\n");
    reset();
}

void notified(microkit_channel channel)
{
    (void)channel;
}
