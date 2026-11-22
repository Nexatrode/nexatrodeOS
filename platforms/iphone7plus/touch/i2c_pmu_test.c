#include "i2c_pmu.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    uint32_t regs[0x48 / 4];
    uint32_t tx_words[32];
    unsigned tx_count;
    unsigned write_bytes, expected_register_bytes;
    int reading, nack, stalled, ignore_value_write;
    uint8_t data;
} fake_i2c;

static uint32_t fake_read(void *context, uint32_t offset)
{
    fake_i2c *bus = context;
    if (offset == 0x28)
        return 3; /* The live D11 controller revision. */
    if (offset == 0x14)
        return bus->nack ? 0x00200000u :
               bus->stalled ? 0 :
               bus->reading ? 0x00100000u : 0x08000000u;
    if (offset == 0x08)
        return bus->reading && !bus->stalled ? 0x100u : 0;
    if (offset == 0x04)
        return bus->data;
    return bus->regs[offset / 4];
}

static void fake_write(void *context, uint32_t offset, uint32_t value)
{
    fake_i2c *bus = context;
    if (offset == 0x00) {
        assert(bus->tx_count < 32);
        bus->tx_words[bus->tx_count++] = value;
        if (value & 0x400u)
            bus->reading = 1;
        else if (value & 0x100u) {
            bus->reading = 0;
            bus->write_bytes = 0;
        } else {
            bus->write_bytes++;
            if ((value & 0x200u) &&
                bus->write_bytes == bus->expected_register_bytes + 1 &&
                !bus->ignore_value_write)
                bus->data = (uint8_t)value;
        }
    } else if (offset != 0x14) {
        bus->regs[offset / 4] = value;
    }
}

int main(void)
{
    fake_i2c bus = { .data = 0x95, .expected_register_bytes = 1 };
    touch_i2c_io io = { &bus, fake_read, fake_write };
    uint8_t value = 0;

    assert(touch_i2c_pmu_read(&io, 0x27, 0x05, 1, 8, &value)
           == TOUCH_I2C_OK);
    assert(value == 0x95);
    assert(bus.regs[0x1c / 4] == 15);
    assert(bus.tx_count == 4);
    assert(bus.tx_words[0] == 0x14e);
    assert(bus.tx_words[1] == 0x205);
    assert(bus.tx_words[2] == 0x14f);
    assert(bus.tx_words[3] == 0x601);

    memset(&bus, 0, sizeof(bus));
    bus.data = 1;
    bus.expected_register_bytes = 2;
    assert(touch_i2c_pmu_read(&io, 0x74, 0x031f, 2, 8, &value)
           == TOUCH_I2C_OK);
    assert(value == 1);
    assert(bus.tx_count == 5);
    assert(bus.tx_words[0] == 0x1e8);
    assert(bus.tx_words[1] == 0x03);
    assert(bus.tx_words[2] == 0x21f);
    assert(bus.tx_words[3] == 0x1e9);
    assert(bus.tx_words[4] == 0x601);

    memset(&bus, 0, sizeof(bus));
    bus.nack = 1;
    bus.expected_register_bytes = 1;
    assert(touch_i2c_pmu_read(&io, 0x27, 5, 1, 8, &value)
           == TOUCH_I2C_NACK);
    assert(bus.regs[0x10 / 4] == 0x80000000u);
    memset(&bus, 0, sizeof(bus));
    bus.stalled = 1;
    bus.expected_register_bytes = 1;
    assert(touch_i2c_pmu_read(&io, 0x27, 5, 1, 8, &value)
           == TOUCH_I2C_TIMEOUT);
    assert(touch_i2c_pmu_read(&io, 0x27, 0x100, 1, 8, &value)
           == TOUCH_I2C_INVALID);

    memset(&bus, 0, sizeof(bus));
    bus.data = 0x0f;
    bus.expected_register_bytes = 1;
    uint8_t before = 0, after = 0;
    assert(touch_i2c_pmu_update(&io, 0x27, 5, 1, 0x10, 0x10, 8,
                                &before, &after) == TOUCH_I2C_OK);
    assert(before == 0x0f && after == 0x1f && bus.data == 0x1f);
    assert(bus.tx_words[5] == 0x05);
    assert(bus.tx_words[6] == 0x21f);
    unsigned prior_words = bus.tx_count;
    assert(touch_i2c_pmu_update(&io, 0x27, 5, 1, 0x10, 0x10, 8,
                                &before, &after) == TOUCH_I2C_OK);
    assert(bus.tx_count == prior_words + 4); /* Read only on no-op. */

    memset(&bus, 0, sizeof(bus));
    bus.data = 0x0f;
    bus.expected_register_bytes = 1;
    bus.ignore_value_write = 1;
    assert(touch_i2c_pmu_update(&io, 0x27, 5, 1, 0x10, 0x10, 8,
                                &before, &after) == TOUCH_I2C_VERIFY_ERROR);
    assert(after == 0x0f);

    puts("D11 PMU I2C read/update: PASS");
    return 0;
}
