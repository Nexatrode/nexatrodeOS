#include "i2c_pmu.h"

enum {
    MTXFIFO = 0x00, MRXFIFO = 0x04, FIFOLVL = 0x08, RESET = 0x10,
    SMSTA = 0x14, IRQMASK = 0x18, CTL = 0x1c, REV = 0x28,
    RDCOUNT = 0x2c, FILTER = 0x38, LOCK = 0x44,
    FIFO_START = 0x100, FIFO_STOP = 0x200, FIFO_READ = 0x400,
    STATUS_XEN = 0x08000000, STATUS_NACK = 0x00200000,
    STATUS_RXDONE = 0x00100000, STATUS_ERROR = 0x00800040,
    STATUS_CLEAR = STATUS_XEN | STATUS_NACK | STATUS_ERROR,
    RESET_BIT = 0x80000000
};

static void abort_transfer(const touch_i2c_io *io)
{
    io->write(io->context, IRQMASK, 0);
    io->write(io->context, SMSTA, STATUS_CLEAR);
    io->write(io->context, RESET, RESET_BIT);
}

static int wait_for_status(const touch_i2c_io *io, uint32_t wanted,
                           unsigned poll_limit)
{
    for (unsigned i = 0; i < poll_limit; i++) {
        uint32_t status = io->read(io->context, SMSTA);
        if (status & STATUS_ERROR)
            return TOUCH_I2C_BUS_ERROR;
        if (status & STATUS_NACK)
            return TOUCH_I2C_NACK;
        if (status & wanted) {
            io->write(io->context, SMSTA, status);
            return TOUCH_I2C_OK;
        }
    }
    return TOUCH_I2C_TIMEOUT;
}

static void prepare_bus(const touch_i2c_io *io)
{
    uint32_t divider;
    /* D11's PMU buses have a 24 MHz parent and default to 100 kHz.
     * The Hx divider is ceil(parent / (16 * bus speed)) = 15. */
    divider = 15;
    if (io->read(io->context, REV) >= 6)
        divider |= 0x800;
    io->write(io->context, CTL, divider);
    io->write(io->context, SMSTA, STATUS_CLEAR);
    io->write(io->context, IRQMASK, 0);
    io->write(io->context, RESET, RESET_BIT);
    io->write(io->context, FILTER,
              io->read(io->context, FILTER) | 0x200u);
}

static int valid_request(const touch_i2c_io *io, uint8_t device_address,
                         uint16_t register_address, unsigned register_bytes,
                         unsigned poll_limit)
{
    return io && io->read && io->write && poll_limit &&
           device_address <= 0x7f &&
           (register_bytes == 1 || register_bytes == 2) &&
           (register_bytes != 1 || register_address <= 0xff);
}

int touch_i2c_pmu_read(const touch_i2c_io *io, uint8_t device_address,
                       uint16_t register_address, unsigned register_bytes,
                       unsigned poll_limit, uint8_t *value)
{
    uint32_t rdcount;
    int result;
    if (!value || !valid_request(io, device_address, register_address,
                                 register_bytes, poll_limit))
        return TOUCH_I2C_INVALID;

    prepare_bus(io);

    io->write(io->context, LOCK, 1);
    io->write(io->context, MTXFIFO,
              FIFO_START | ((uint32_t)device_address << 1));
    if (register_bytes == 2)
        io->write(io->context, MTXFIFO, register_address >> 8);
    io->write(io->context, MTXFIFO,
              FIFO_STOP | (register_address & 0xffu));
    io->write(io->context, LOCK, 0);
    result = wait_for_status(io, STATUS_XEN, poll_limit);
    if (result != TOUCH_I2C_OK) {
        abort_transfer(io);
        return result;
    }

    rdcount = io->read(io->context, RDCOUNT);
    io->write(io->context, RDCOUNT, (rdcount & ~0xff00u) | 0x100u);
    io->write(io->context, LOCK, 1);
    io->write(io->context, MTXFIFO,
              FIFO_START | ((uint32_t)device_address << 1) | 1u);
    io->write(io->context, MTXFIFO, FIFO_READ | FIFO_STOP | 1u);
    io->write(io->context, LOCK, 0);
    for (unsigned i = 0; i < poll_limit; i++) {
        uint32_t status = io->read(io->context, SMSTA);
        if (status & STATUS_ERROR) {
            result = TOUCH_I2C_BUS_ERROR;
            break;
        }
        if (status & STATUS_NACK) {
            result = TOUCH_I2C_NACK;
            break;
        }
        if ((io->read(io->context, FIFOLVL) & 0xff00u) >= 0x100u) {
            *value = (uint8_t)io->read(io->context, MRXFIFO);
            io->write(io->context, SMSTA, status);
            return TOUCH_I2C_OK;
        }
    }
    if (result == TOUCH_I2C_OK)
        result = TOUCH_I2C_TIMEOUT;
    abort_transfer(io);
    return result;
}

int touch_i2c_pmu_update(const touch_i2c_io *io, uint8_t device_address,
                         uint16_t register_address, unsigned register_bytes,
                         uint8_t mask, uint8_t set_bits, unsigned poll_limit,
                         uint8_t *before, uint8_t *after)
{
    uint8_t old_value, desired, observed;
    int result;
    if (!before || !after || !mask || (set_bits & ~mask) ||
        !valid_request(io, device_address, register_address,
                       register_bytes, poll_limit))
        return TOUCH_I2C_INVALID;
    result = touch_i2c_pmu_read(io, device_address, register_address,
                                register_bytes, poll_limit, &old_value);
    if (result != TOUCH_I2C_OK)
        return result;
    *before = old_value;
    desired = (old_value & (uint8_t)~mask) | set_bits;
    if (desired == old_value) {
        *after = old_value;
        return TOUCH_I2C_OK;
    }

    prepare_bus(io);
    io->write(io->context, LOCK, 1);
    io->write(io->context, MTXFIFO,
              FIFO_START | ((uint32_t)device_address << 1));
    if (register_bytes == 2)
        io->write(io->context, MTXFIFO, register_address >> 8);
    io->write(io->context, MTXFIFO, register_address & 0xffu);
    io->write(io->context, MTXFIFO, FIFO_STOP | desired);
    io->write(io->context, LOCK, 0);
    result = wait_for_status(io, STATUS_XEN, poll_limit);
    if (result != TOUCH_I2C_OK) {
        abort_transfer(io);
        return result;
    }

    result = touch_i2c_pmu_read(io, device_address, register_address,
                                register_bytes, poll_limit, &observed);
    if (result != TOUCH_I2C_OK)
        return result;
    *after = observed;
    if ((observed & mask) != (desired & mask))
        return TOUCH_I2C_VERIFY_ERROR;
    return TOUCH_I2C_OK;
}
