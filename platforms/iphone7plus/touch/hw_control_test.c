#include "hw_control.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    uint32_t regs[1024];
    uint32_t log_values[8];
    uint32_t log_offsets[8];
    unsigned writes, delays;
    int gate_stuck, clock_stuck;
} fake_hw;

static uint32_t fake_read(void *context, uint32_t offset)
{
    fake_hw *hw = context;
    return hw->regs[offset / 4];
}

static void fake_write(void *context, uint32_t offset, uint32_t value)
{
    fake_hw *hw = context;
    if (hw->writes < 8) {
        hw->log_offsets[hw->writes] = offset;
        hw->log_values[hw->writes] = value;
    }
    hw->writes++;
    if (offset == TOUCH_SPI2_GATE_OFFSET && !hw->gate_stuck &&
        (value & 0xfu) == 0xfu)
        value |= 0xf0u;
    if (offset == TOUCH_CLOCK_OFFSET && hw->clock_stuck &&
        (value & (1u << 19)))
        value |= 1u << 18;
    hw->regs[offset / 4] = value;
}

static void fake_delay(void *context, unsigned microseconds)
{
    fake_hw *hw = context;
    assert(microseconds == 10 || microseconds == 100);
    hw->delays++;
}

int main(void)
{
    fake_hw hw = {0};
    touch_hw_io io = { &hw, fake_read, fake_write, fake_delay };
    assert(touch_hw_enable_gate(&io, TOUCH_SPI2_GATE_OFFSET) == TOUCH_HW_OK);
    assert((hw.regs[TOUCH_SPI2_GATE_OFFSET / 4] & 0x100000ffu)
           == 0x100000ffu);
    hw.gate_stuck = 1;
    hw.regs[TOUCH_SPI2_GATE_OFFSET / 4] = 0;
    assert(touch_hw_enable_gate(&io, TOUCH_SPI2_GATE_OFFSET)
           == TOUCH_HW_TIMEOUT);

    memset(&hw, 0, sizeof(hw));
    hw.regs[0] = 1u << 31;
    assert(touch_hw_enable_clock(&io) == TOUCH_HW_OK);
    assert((hw.regs[0] & 1023u) == 732u);
    assert((hw.regs[0] & (1u << 19)) != 0);
    assert((hw.regs[0] & (1u << 31)) == 0);
    assert(touch_hw_disable_clock(&io) == TOUCH_HW_OK);
    assert(!(hw.regs[0] & (1u << 19)));
    assert(hw.regs[0] & (1u << 31));
    hw.clock_stuck = 1;
    hw.delays = 0;
    assert(touch_hw_enable_clock(&io) == TOUCH_HW_TIMEOUT);
    assert(hw.delays == 5000);

    memset(&hw, 0, sizeof(hw));
    assert(touch_hw_gpio_output(&io, TOUCH_RESET_GPIO, 0) == TOUCH_HW_OK);
    assert(hw.writes == 2);
    assert(hw.log_values[0] == 2u);
    assert(hw.log_values[1] == (1u << 9 | 2u));
    assert(touch_hw_gpio_output(&io, TOUCH_RESET_GPIO, 1) == TOUCH_HW_OK);
    assert(hw.writes == 3);
    assert(hw.regs[TOUCH_RESET_GPIO] == (1u << 9 | 2u | 1u));
    assert(touch_hw_gpio_peripheral(&io, 197) == TOUCH_HW_OK);
    assert(hw.regs[197] & (1u << 5));
    assert(touch_hw_gpio_output(&io, 224, 1) == TOUCH_HW_INVALID);

    memset(&hw, 0, sizeof(hw));
    hw.regs[TOUCH_IRQ_GPIO] = 0x70020u;
    assert(touch_hw_gpio_irq_falling(&io, TOUCH_IRQ_GPIO) == TOUCH_HW_OK);
    assert(hw.regs[TOUCH_IRQ_GPIO] == ((1u << 9) | (5u << 1)));
    assert(touch_hw_gpio_irq_pending(&io, TOUCH_IRQ_GPIO) == 0);
    hw.regs[0x814 / 4] = 1u << (TOUCH_IRQ_GPIO & 31u);
    assert(touch_hw_gpio_irq_pending(&io, TOUCH_IRQ_GPIO) == 1);
    assert(touch_hw_gpio_irq_ack(&io, TOUCH_IRQ_GPIO) == TOUCH_HW_OK);
    assert(hw.log_offsets[2] == 0x814u);
    assert(hw.log_values[2] == (1u << (TOUCH_IRQ_GPIO & 31u)));

    puts("D11 touch PMGR/GPIO control: PASS");
    return 0;
}
