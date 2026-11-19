#ifndef IPHONE7PLUS_TOUCH_I2C_PMU_H
#define IPHONE7PLUS_TOUCH_I2C_PMU_H

#include <stdint.h>

typedef struct {
    void *context;
    uint32_t (*read)(void *context, uint32_t offset);
    void (*write)(void *context, uint32_t offset, uint32_t value);
} touch_i2c_io;

enum {
    TOUCH_I2C_OK = 0,
    TOUCH_I2C_INVALID = -1,
    TOUCH_I2C_TIMEOUT = -2,
    TOUCH_I2C_NACK = -3,
    TOUCH_I2C_BUS_ERROR = -4,
    TOUCH_I2C_VERIFY_ERROR = -5
};

/* Select a PMU register, then read one byte. The register selection is an
 * I2C address write followed by a separate read, as in the Sandcastle bus
 * driver. It never writes a PMU register value. register_bytes is 1 or 2.
 */
int touch_i2c_pmu_read(const touch_i2c_io *io, uint8_t device_address,
                       uint16_t register_address, unsigned register_bytes,
                       unsigned poll_limit, uint8_t *value);

/* Change only mask bits, preserving all unrelated PMU controls. Readback
 * verifies the selected bits. A no-op update issues no value write. */
int touch_i2c_pmu_update(const touch_i2c_io *io, uint8_t device_address,
                         uint16_t register_address, unsigned register_bytes,
                         uint8_t mask, uint8_t set_bits, unsigned poll_limit,
                         uint8_t *before, uint8_t *after);

#endif
