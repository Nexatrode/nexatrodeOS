#elif defined(CONFIG_PLAT_SUNFISH)
#define UART_BASE 0x00a88000
#define SE_UART_TX_TRANS_LEN 0x270
#define SE_GENI_M_CMD0 0x600
#define SE_GENI_M_IRQ_STATUS 0x610
#define SE_GENI_M_IRQ_CLEAR 0x618
#define SE_GENI_TX_FIFOn 0x700
#define M_CMD_DONE (1u << 0)

void uart_init(void) {}

void putc(uint8_t ch)
{
    *UART_REG(SE_GENI_M_IRQ_CLEAR) = M_CMD_DONE;
    *UART_REG(SE_UART_TX_TRANS_LEN) = 1;
    *UART_REG(SE_GENI_M_CMD0) = (1u << 27);
    *UART_REG(SE_GENI_TX_FIFOn) = ch;
    for (unsigned spin = 0; spin < 0xfffff; spin++) {
        if (*UART_REG(SE_GENI_M_IRQ_STATUS) & M_CMD_DONE) return;
    }
}
