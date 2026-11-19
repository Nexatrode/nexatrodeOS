/* GENI serial-engine offsets for early sunfish debug UART putchar.
 *
 * Matches common Linux qcom-geni-se.h / qcom_geni_serial.c naming. The SE base
 * for Pixel 4a uart8 is 0x00a88000 (see docs/pixel-p0-assets/sm7150.dtsi).
 *
 * This header is a desk scaffold only — not linked by ./dev build.
 */

#ifndef PERSONAL_OS_SUNFISH_GENI_UART_H
#define PERSONAL_OS_SUNFISH_GENI_UART_H

#define SUNFISH_GENI_UART_BASE 0x00a88000u
#define SUNFISH_GENI_UART_SIZE 0x4000u
#define SUNFISH_GENI_UART_SPI 355u

#define SE_GENI_STATUS 0x40u
#define SE_UART_TX_TRANS_LEN 0x270u
#define SE_GENI_M_CMD0 0x600u
#define SE_GENI_M_IRQ_STATUS 0x610u
#define SE_GENI_M_IRQ_CLEAR 0x618u
#define SE_GENI_TX_FIFOn 0x700u
#define SE_GENI_TX_FIFO_STATUS 0x800u

#define SUNFISH_UART_START_TX_CMD (1u << 27)
#define SUNFISH_M_CMD_DONE (1u << 0)

#endif
