/*
 * Polling GENI UART putchar stub for Pixel 4a (sunfish) P1 planning.
 *
 * NOT part of the QEMU/Microkit build yet. The per-byte TX command sequence
 * was validated with a temporary boot on 2026-10-06.
 *
 * Contract: the bootloader (after `fastboot oem uart enable`) leaves uart8
 * clocks/pins/baud configured. This stub starts a one-byte GENI TX command,
 * writes the byte, and waits for M_CMD_DONE (as Linux's polling console does).
 * Full SE firmware load, clock enable, and baud programming are deliberately
 * omitted — those belong in a real platform port once serial capture proves
 * the debug path.
 */

#include "geni_uart.h"

#include <stdint.h>

static inline volatile uint32_t *geni_reg(uintptr_t offset)
{
    return (volatile uint32_t *)(uintptr_t)(SUNFISH_GENI_UART_BASE + offset);
}

/*
 * Blocking putchar. Returns 0 on the happy path.
 * Returns -1 on timeout (clocks off / wrong base / hung SE).
 */
int sunfish_geni_putchar(int ch)
{
    const int kMaxSpins = 0xfffff;

    *geni_reg(SE_GENI_M_IRQ_CLEAR) = SUNFISH_M_CMD_DONE;
    *geni_reg(SE_UART_TX_TRANS_LEN) = 1u;
    *geni_reg(SE_GENI_M_CMD0) = SUNFISH_UART_START_TX_CMD;
    *geni_reg(SE_GENI_TX_FIFOn) = (uint32_t)(ch & 0xff);
    for (int spins = 0; spins < kMaxSpins; spins++) {
        if (*geni_reg(SE_GENI_M_IRQ_STATUS) & SUNFISH_M_CMD_DONE) {
            return 0;
        }
    }
    return -1;
}

void sunfish_geni_puts(const char *text)
{
    if (text == 0) {
        return;
    }
    for (; *text != '\0'; text++) {
        if (*text == '\n') {
            (void)sunfish_geni_putchar('\r');
        }
        (void)sunfish_geni_putchar((unsigned char)*text);
    }
}

/* Optional smoke entry for a future elfloader console hook. */
void sunfish_geni_hello(void)
{
    sunfish_geni_puts("sunfish: geni uart stub ready\n");
}
