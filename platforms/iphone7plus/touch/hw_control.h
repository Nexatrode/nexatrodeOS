#ifndef IPHONE7PLUS_TOUCH_HW_CONTROL_H
#define IPHONE7PLUS_TOUCH_HW_CONTROL_H

#include <stdint.h>

/* Offsets are relative to the mapped PMGR or GPIO region supplied by caller. */
typedef struct {
    void *context;
    uint32_t (*read)(void *context, uint32_t offset);
    void (*write)(void *context, uint32_t offset, uint32_t value);
    void (*delay_us)(void *context, unsigned microseconds);
} touch_hw_io;

enum {
    TOUCH_HW_OK = 0,
    TOUCH_HW_INVALID = -1,
    TOUCH_HW_TIMEOUT = -2,
    TOUCH_SPI2_GATE_OFFSET = 0x1d8,
    TOUCH_CLOCK_OFFSET = 0x0,
    TOUCH_RESET_GPIO = 161,
    TOUCH_CS_GPIO = 81,
    TOUCH_IRQ_GPIO = 166
};

int touch_hw_enable_gate(const touch_hw_io *pmgr, uint32_t offset);
int touch_hw_enable_clock(const touch_hw_io *touch_clock);
int touch_hw_disable_clock(const touch_hw_io *touch_clock);
int touch_hw_gpio_output(const touch_hw_io *gpio, unsigned pin, int high);
int touch_hw_gpio_peripheral(const touch_hw_io *gpio, unsigned pin);
int touch_hw_gpio_irq_falling(const touch_hw_io *gpio, unsigned pin);
int touch_hw_gpio_irq_pending(const touch_hw_io *gpio, unsigned pin);
int touch_hw_gpio_irq_ack(const touch_hw_io *gpio, unsigned pin);

#endif
