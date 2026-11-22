#include "spi2_transport.h"

enum {
    CLKCFG = 0x00, CONFIG = 0x04, STATUS = 0x08, PIN = 0x0c,
    TXDATA = 0x10, RXDATA = 0x20, CLKDIV = 0x30, RXCNT = 0x34,
    CLKIDLE = 0x38, TXCNT = 0x4c,
    CONFIG_SET = 0x0010401e, PIOEN = 1u << 5,
    STATUS_CLEAR = (1u << 22) | (1u << 1) | 1u,
    FIFO_SIZE = 16
};

static int valid_io(const touch_spi2_io *io)
{
    return io && io->read && io->write;
}

int touch_spi2_prepare(const touch_spi2_io *io, uint32_t clock_hz,
                       uint32_t speed_hz)
{
    uint32_t divider;
    if (!valid_io(io) || !clock_hz || !speed_hz || speed_hz > 3000000)
        return TOUCH_SPI2_INVALID;
    /* Ceiling division without overflowing a 32-bit clock value. */
    divider = clock_hz / speed_hz + (clock_hz % speed_hz != 0);
    if (divider < 2)
        divider = 2;
    if (divider > 2048)
        return TOUCH_SPI2_INVALID;

    io->write(io->context, CLKCFG, 0);
    io->write(io->context, PIN, 0);
    io->write(io->context, STATUS, STATUS_CLEAR);
    io->write(io->context, CLKDIV, divider);
    io->write(io->context, CLKIDLE, 2);
    io->write(io->context, STATUS, STATUS_CLEAR);
    io->write(io->context, CONFIG, CONFIG_SET);
    io->write(io->context, CLKCFG, 0x0d);
    (void)io->read(io->context, CONFIG);
    return TOUCH_SPI2_OK;
}

int touch_spi2_transfer(const touch_spi2_io *io, const uint8_t *tx,
                        uint8_t *rx, size_t length, unsigned poll_limit)
{
    size_t sent = 0, received = 0;
    int result = TOUCH_SPI2_TIMEOUT;
    if (!valid_io(io) || !length || length > TOUCH_SPI2_MAX_TRANSFER ||
        !poll_limit)
        return TOUCH_SPI2_INVALID;

    io->write(io->context, RXCNT, (uint32_t)length);
    io->write(io->context, TXCNT, (uint32_t)length);
    io->write(io->context, CONFIG, CONFIG_SET | PIOEN);

    for (unsigned poll = 0; poll < poll_limit; poll++) {
        uint32_t status = io->read(io->context, STATUS);
        unsigned rx_count = (status >> 11) & 31u;
        unsigned tx_count = (status >> 6) & 31u;
        if (rx_count > FIFO_SIZE || tx_count > FIFO_SIZE ||
            rx_count > length - received || rx_count > sent - received) {
            result = TOUCH_SPI2_FIFO_ERROR;
            break;
        }
        while (rx_count--) {
            uint8_t byte = (uint8_t)io->read(io->context, RXDATA);
            if (rx)
                rx[received] = byte;
            received++;
        }
        if (received == length) {
            result = TOUCH_SPI2_OK;
            break;
        }
        while (sent < length && tx_count < FIFO_SIZE) {
            io->write(io->context, TXDATA, tx ? tx[sent] : 0);
            sent++;
            tx_count++;
        }
    }

    /* Always stop PIO and clear completion/ready status on exit. */
    io->write(io->context, CONFIG, CONFIG_SET);
    io->write(io->context, STATUS, STATUS_CLEAR);
    return result;
}
