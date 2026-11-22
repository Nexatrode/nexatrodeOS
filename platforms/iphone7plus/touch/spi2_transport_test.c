#include "spi2_transport.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    uint32_t regs[0x50 / 4];
    uint8_t fifo[16];
    unsigned head, count, writes, reads;
    int stall, bad_fifo;
} fake_spi;

static uint32_t fake_read(void *context, uint32_t offset)
{
    fake_spi *spi = context;
    if (offset == 0x08)
        return spi->bad_fifo ? (17u << 11) : spi->count << 11;
    if (offset == 0x20) {
        uint8_t value;
        assert(spi->count);
        value = spi->fifo[spi->head];
        spi->head = (spi->head + 1) % 16;
        spi->count--;
        spi->reads++;
        return value;
    }
    return spi->regs[offset / 4];
}

static void fake_write(void *context, uint32_t offset, uint32_t value)
{
    fake_spi *spi = context;
    if (offset == 0x10) {
        spi->writes++;
        if (!spi->stall) {
            assert(spi->count < 16);
            spi->fifo[(spi->head + spi->count) % 16] = (uint8_t)value;
            spi->count++;
        }
    } else {
        spi->regs[offset / 4] = value;
    }
}

int main(void)
{
    fake_spi spi = {0};
    touch_spi2_io io = { &spi, fake_read, fake_write };
    uint8_t tx[40], rx[40];

    assert(touch_spi2_prepare(&io, 24000000, 3000000) == TOUCH_SPI2_OK);
    assert(spi.regs[0x30 / 4] == 8);
    assert(spi.regs[0x00 / 4] == 0x0d);
    assert(touch_spi2_prepare(&io, 0, 3000000) == TOUCH_SPI2_INVALID);
    assert(touch_spi2_prepare(&io, 24000000, 3000001) == TOUCH_SPI2_INVALID);
    assert(touch_spi2_prepare(&io, 24000000, 1) == TOUCH_SPI2_INVALID);

    for (unsigned i = 0; i < sizeof(tx); i++)
        tx[i] = (uint8_t)(i ^ 0xa5);
    memset(rx, 0, sizeof(rx));
    assert(touch_spi2_transfer(&io, tx, rx, sizeof(tx), 16) == TOUCH_SPI2_OK);
    assert(!memcmp(tx, rx, sizeof(tx)));
    assert(spi.writes == 40 && spi.reads == 40);
    assert(spi.regs[0x04 / 4] == 0x0010401e);

    assert(touch_spi2_transfer(&io, NULL, rx, 1, 4) == TOUCH_SPI2_OK);
    assert(rx[0] == 0);
    assert(touch_spi2_transfer(&io, tx, NULL, 1, 4) == TOUCH_SPI2_OK);
    assert(touch_spi2_transfer(&io, tx, rx, 0, 4) == TOUCH_SPI2_INVALID);
    assert(touch_spi2_transfer(&io, tx, rx, TOUCH_SPI2_MAX_TRANSFER + 1, 4)
           == TOUCH_SPI2_INVALID);

    spi.stall = 1;
    assert(touch_spi2_transfer(&io, tx, rx, 1, 3) == TOUCH_SPI2_TIMEOUT);
    assert(spi.regs[0x04 / 4] == 0x0010401e);
    spi.stall = 0;
    spi.bad_fifo = 1;
    assert(touch_spi2_transfer(&io, tx, rx, 1, 3) == TOUCH_SPI2_FIFO_ERROR);
    assert(spi.regs[0x04 / 4] == 0x0010401e);
    spi.bad_fifo = 0;
    spi.count = 1; /* A stale reply must not be accepted as this transfer. */
    assert(touch_spi2_transfer(&io, tx, rx, 1, 3) == TOUCH_SPI2_FIFO_ERROR);

    puts("D11 SPI2 polling transport: PASS");
    return 0;
}
