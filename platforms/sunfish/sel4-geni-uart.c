/* seL4 kernel serial driver for Pixel 4a's bootloader-initialised GENI UART. */
#include <config.h>
#include <stdint.h>
#include <machine/io.h>

#define UART_REG(off) ((volatile uint32_t *)(UART_PPTR + (off)))
#define SE_UART_TX_TRANS_LEN 0x270u
#define SE_GENI_M_CMD0 0x600u
#define SE_GENI_M_IRQ_STATUS 0x610u
#define SE_GENI_M_IRQ_CLEAR 0x618u
#define SE_GENI_TX_FIFOn 0x700u
#define M_CMD_DONE (1u << 0)
#define UART_START_TX_CMD (1u << 27)

#ifdef CONFIG_PRINTING
void uart_drv_putchar(unsigned char c)
{
    *UART_REG(SE_GENI_M_IRQ_CLEAR) = M_CMD_DONE;
    *UART_REG(SE_UART_TX_TRANS_LEN) = 1;
    *UART_REG(SE_GENI_M_CMD0) = UART_START_TX_CMD;
    *UART_REG(SE_GENI_TX_FIFOn) = c;
    for (unsigned spin = 0; spin < 0xfffff; spin++) {
        if (*UART_REG(SE_GENI_M_IRQ_STATUS) & M_CMD_DONE) {
            return;
        }
    }
}
#endif

#ifdef CONFIG_DEBUG_BUILD
unsigned char uart_drv_getchar(void)
{
    /* Receive is not wired into the first UART-only bring-up. */
    return 0;
}
#endif
