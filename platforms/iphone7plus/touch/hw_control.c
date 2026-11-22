#include "hw_control.h"

enum {
    GPIO_DATA = 1u,
    GPIO_IRQ_MASK = 7u << 1,
    GPIO_IRQ_OUT = 1u << 1,
    GPIO_IRQ_FALLING = 5u << 1,
    GPIO_PERIPHERAL = 1u << 5,
    GPIO_CFG_DONE = 1u << 9,
    GPIO_GROUP_MASK = 7u << 16,
    CLOCK_DISABLE = 1u << 31,
    CLOCK_ENABLE = 1u << 19,
    CLOCK_BUSY = 1u << 18,
    CLOCK_DIV_MASK = 1023u
};

static int valid_io(const touch_hw_io *io)
{
    return io && io->read && io->write;
}

int touch_hw_enable_gate(const touch_hw_io *pmgr, uint32_t offset)
{
    uint32_t value;
    if (!valid_io(pmgr) || (offset & 3u))
        return TOUCH_HW_INVALID;
    value = pmgr->read(pmgr->context, offset);
    pmgr->write(pmgr->context, offset, value | 0x0fu);
    for (unsigned poll = 0; poll < 10000; poll++) {
        value = pmgr->read(pmgr->context, offset);
        if (((value >> 4) & 0x0fu) == 0x0fu) {
            pmgr->write(pmgr->context, offset, value | 0x10000000u);
            return TOUCH_HW_OK;
        }
    }
    return TOUCH_HW_TIMEOUT;
}

int touch_hw_enable_clock(const touch_hw_io *touch_clock)
{
    /* The D11 touch clock is a rounded 24 MHz / 32,768 Hz divider. */
    const uint32_t divider = (24000000u + 32768u / 2u) / 32768u;
    uint32_t value;
    if (!valid_io(touch_clock) || !touch_clock->delay_us)
        return TOUCH_HW_INVALID;
    value = touch_clock->read(touch_clock->context, TOUCH_CLOCK_OFFSET);
    value &= ~CLOCK_DISABLE;
    touch_clock->write(touch_clock->context, TOUCH_CLOCK_OFFSET, value);
    value = (value & ~CLOCK_DIV_MASK) | CLOCK_ENABLE | divider;
    touch_clock->write(touch_clock->context, TOUCH_CLOCK_OFFSET, value);
    for (unsigned poll = 0; poll < 5000; poll++) {
        value = touch_clock->read(touch_clock->context, TOUCH_CLOCK_OFFSET);
        if (!(value & CLOCK_BUSY))
            return TOUCH_HW_OK;
        touch_clock->delay_us(touch_clock->context, 10);
    }
    return TOUCH_HW_TIMEOUT;
}

int touch_hw_disable_clock(const touch_hw_io *touch_clock)
{
    uint32_t value;
    int result = TOUCH_HW_TIMEOUT;
    if (!valid_io(touch_clock) || !touch_clock->delay_us)
        return TOUCH_HW_INVALID;
    value = touch_clock->read(touch_clock->context, TOUCH_CLOCK_OFFSET);
    touch_clock->write(touch_clock->context, TOUCH_CLOCK_OFFSET,
                       value & ~CLOCK_ENABLE);
    for (unsigned poll = 0; poll < 5000; poll++) {
        value = touch_clock->read(touch_clock->context, TOUCH_CLOCK_OFFSET);
        if (!(value & CLOCK_BUSY)) {
            result = TOUCH_HW_OK;
            break;
        }
        touch_clock->delay_us(touch_clock->context, 10);
    }
    /* Request disable even if the busy indication did not settle. */
    touch_clock->write(touch_clock->context, TOUCH_CLOCK_OFFSET,
                       (value & ~CLOCK_ENABLE) | CLOCK_DISABLE);
    touch_clock->delay_us(touch_clock->context, 100);
    return result;
}

static int gpio_set(const touch_hw_io *gpio, unsigned pin,
                    uint32_t clear_bits, uint32_t set_bits)
{
    uint32_t offset, previous, config;
    if (!valid_io(gpio) || pin >= 224)
        return TOUCH_HW_INVALID;
    offset = pin * 4;
    previous = gpio->read(gpio->context, offset);
    config = (previous & ~clear_bits) | set_bits;
    /* Match the A10 pinctrl driver's two-step initial CFG_DONE commit. */
    if (!(previous & GPIO_CFG_DONE))
        gpio->write(gpio->context, offset, config & ~GPIO_CFG_DONE);
    gpio->write(gpio->context, offset, config);
    return TOUCH_HW_OK;
}

int touch_hw_gpio_output(const touch_hw_io *gpio, unsigned pin, int high)
{
    return gpio_set(gpio, pin, GPIO_IRQ_MASK | GPIO_DATA | GPIO_PERIPHERAL,
                    GPIO_CFG_DONE | GPIO_IRQ_OUT | (high ? GPIO_DATA : 0));
}

int touch_hw_gpio_peripheral(const touch_hw_io *gpio, unsigned pin)
{
    /* Preserve the current DATA bit, as Sandcastle's pinctrl driver does. */
    uint32_t value;
    if (!valid_io(gpio) || pin >= 224)
        return TOUCH_HW_INVALID;
    value = gpio->read(gpio->context, pin * 4) & GPIO_DATA;
    return gpio_set(gpio, pin, GPIO_IRQ_MASK | GPIO_DATA,
                    GPIO_CFG_DONE | GPIO_PERIPHERAL | value);
}

static uint32_t gpio_irq_offset(unsigned pin)
{
    return 0x800u + 4u * (pin >> 5);
}

int touch_hw_gpio_irq_falling(const touch_hw_io *gpio, unsigned pin)
{
    if (!valid_io(gpio) || pin >= 224)
        return TOUCH_HW_INVALID;
    /* Sandcastle routes D11's active-low GPIO IRQ through group zero. */
    return gpio_set(gpio, pin, GPIO_IRQ_MASK | GPIO_DATA |
                    GPIO_PERIPHERAL | GPIO_GROUP_MASK,
                    GPIO_CFG_DONE | GPIO_IRQ_FALLING);
}

int touch_hw_gpio_irq_pending(const touch_hw_io *gpio, unsigned pin)
{
    if (!valid_io(gpio) || pin >= 224)
        return TOUCH_HW_INVALID;
    return !!(gpio->read(gpio->context, gpio_irq_offset(pin)) &
              (1u << (pin & 31u)));
}

int touch_hw_gpio_irq_ack(const touch_hw_io *gpio, unsigned pin)
{
    if (!valid_io(gpio) || pin >= 224)
        return TOUCH_HW_INVALID;
    gpio->write(gpio->context, gpio_irq_offset(pin), 1u << (pin & 31u));
    return TOUCH_HW_OK;
}
