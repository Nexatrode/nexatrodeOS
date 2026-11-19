#ifndef IPHONE7PLUS_TOUCH_SPI2_TRANSPORT_H
#define IPHONE7PLUS_TOUCH_SPI2_TRANSPORT_H

#include <stddef.h>
#include <stdint.h>

/* The caller enables the SPI2 PMGR gate and owns the touch chip-select GPIO.
 * Each transfer is full duplex; NULL tx sends zeroes and NULL rx discards data.
 */
typedef struct {
    void *context;
    uint32_t (*read)(void *context, uint32_t offset);
    void (*write)(void *context, uint32_t offset, uint32_t value);
} touch_spi2_io;

enum {
    TOUCH_SPI2_OK = 0,
    TOUCH_SPI2_INVALID = -1,
    TOUCH_SPI2_TIMEOUT = -2,
    TOUCH_SPI2_FIFO_ERROR = -3,
    TOUCH_SPI2_MAX_TRANSFER = 16384
};

int touch_spi2_prepare(const touch_spi2_io *io, uint32_t clock_hz,
                       uint32_t speed_hz);
int touch_spi2_transfer(const touch_spi2_io *io, const uint8_t *tx,
                        uint8_t *rx, size_t length, unsigned poll_limit);

#endif
