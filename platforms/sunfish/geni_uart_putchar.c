/*
 * Polling GENI UART putchar stub for Pixel 4a (sunfish) P1 planning.
 *
 * NOT part of the QEMU/Microkit build. Do not flash images that include this
 * until P0 gates pass (SBU UART capture, userdata backup, live slot dumps).
 *
 * Contract: the bootloader (after `fastboot oem uart enable`) is expected to
 * leave uart8 clocks/pins/baud configured. This stub only:
 *   1) waits until the TX FIFO has room, and
 *   2) writes one byte.
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

static int geni_tx_fifo_words(void)
{
    uint32_t status = *geni_reg(SE_GENI_TX_FIFO_STATUS);
    return (int)(status & TX_FIFO_WC_MASK);
}

/*
 * Blocking putchar. Returns 0 on the happy path.
 * Returns -1 if the FIFO never drains (clocks off / wrong base / hung SE).
 */
int sunfish_geni_putchar(int ch)
{
    const int kMaxSpins = 1000000;
    int spins = 0;

    while (geni_tx_fifo_words() >= (int)SUNFISH_GENI_TX_WATERMARK) {
        if (++spins >= kMaxSpins) {
            return -1;
        }
    }

    *geni_reg(SE_GENI_TX_FIFOn) = (uint32_t)(ch & 0xff);
    return 0;
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
