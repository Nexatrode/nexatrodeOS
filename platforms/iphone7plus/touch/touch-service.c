/* Persistent D11 touch service for the iPhone NexatrodeOS shell. */
#include <microkit.h>
#include <stdint.h>
#include "boot_execute.h"
#include "hw_control.h"
#include "i2c_pmu.h"
#include "report.h"
#include "spi2_transport.h"
#include "touch-state.h"
#include "../../slint-frame-wake.h"

_Static_assert(sizeof(IphoneTouchState) <= 0x4000,
               "touch event queue must fit its shared memory region");
#include "z2_report.h"

uintptr_t touch_service_spi2;
uintptr_t touch_service_pmgr;
uintptr_t touch_service_i2c0;
uintptr_t touch_service_clock;
uintptr_t touch_service_gpio;
uintptr_t touch_service_shared_state;
uintptr_t touch_frame_wake_request;

extern const uint8_t touch_firmware_start[];
extern const uint8_t touch_firmware_end[];

typedef struct {
    touch_hw_io gpio;
    touch_spi2_io spi;
    int spi_error;
} ServiceLink;

static uint32_t read32(void *context, uint32_t offset)
{
    volatile uint32_t *base = context;
    return base[offset / 4];
}

static void write32(void *context, uint32_t offset, uint32_t value)
{
    volatile uint32_t *base = context;
    base[offset / 4] = value;
    asm volatile("dsb sy" ::: "memory");
}

static void delay_us(void *context, unsigned microseconds)
{
    uint32_t iterations;
    (void)context;
    /* CNTVCT_EL0 traps on this bring-up kernel. Match the proven probe's
     * conservative minimum-delay loop until a user timer is available. */
    iterations = microseconds * 4000u;
    asm volatile("1: subs %w0, %w0, #1\n"
                 "   b.ne 1b\n"
                 : "+r"(iterations) :: "cc");
}

static void publish(uint32_t status, uint32_t stage, uint32_t packets,
                    uint32_t active, uint32_t finger, uint32_t x, uint32_t y)
{
    volatile IphoneTouchState *state =
        (volatile IphoneTouchState *)touch_service_shared_state;
    state->revision++;
    asm volatile("dmb ish" ::: "memory");
    state->status = status;
    state->stage = stage;
    state->packets = packets;
    state->active = active;
    state->finger = finger;
    state->x = x;
    state->y = y;
    asm volatile("dmb ish" ::: "memory");
    state->revision++;
    if (status == IPHONE_TOUCH_READY) {
        uint32_t next = state->event_write + 1;
        volatile IphoneTouchEvent *event =
            &state->events[(next - 1) % IPHONE_TOUCH_EVENT_CAPACITY];
        event->revision = (next << 1) | 1;
        asm volatile("dmb ish" ::: "memory");
        event->active = active;
        event->finger = finger;
        event->x = x;
        event->y = y;
        asm volatile("dmb ish" ::: "memory");
        event->revision = next << 1;
        asm volatile("dmb ish" ::: "memory");
        state->event_write = next;
    }
    microkit_notify(0);
}

static int select_chip(void *context, int asserted)
{
    ServiceLink *link = context;
    return touch_hw_gpio_output(&link->gpio, TOUCH_CS_GPIO, !asserted);
}

static int spi_transfer(void *context, const uint8_t *tx, uint8_t *rx,
                        size_t length)
{
    ServiceLink *link = context;
    link->spi_error = touch_spi2_prepare(&link->spi, 24000000, 3000000);
    if (link->spi_error != TOUCH_SPI2_OK) return 1;
    link->spi_error = touch_spi2_transfer(&link->spi, tx, rx,
                                          length, 5000000);
    return link->spi_error != TOUCH_SPI2_OK;
}

static int boot_transfer(void *context, const uint8_t *data, size_t length)
{
    return spi_transfer(context, data, 0, length);
}

static int wait_irq(void *context, unsigned timeout_ms)
{
    ServiceLink *link = context;
    for (unsigned elapsed = 0; elapsed <= timeout_ms; elapsed++) {
        int pending = touch_hw_gpio_irq_pending(&link->gpio, TOUCH_IRQ_GPIO);
        uint32_t pin = read32(link->gpio.context, TOUCH_IRQ_GPIO * 4u);
        if (pending || !(pin & 1u)) {
            if (pending)
                touch_hw_gpio_irq_ack(&link->gpio, TOUCH_IRQ_GPIO);
            return 0;
        }
        delay_us(0, 1000);
    }
    return -1;
}

static void boot_progress(void *context, uint32_t record, uint32_t type)
{
    (void)context;
    (void)record;
    (void)type;
}

static int sync_four(ServiceLink *link)
{
    uint8_t tx[4] = {0}, rx[4] = {0};
    if (select_chip(link, 1)) return -1;
    delay_us(0, 1000);
    int result = spi_transfer(link, tx, rx, sizeof(tx));
    if (select_chip(link, 0)) result = -1;
    return result;
}

void init(void)
{
    const touch_hw_io pmgr = {
        (void *)touch_service_pmgr, read32, write32, 0
    };
    const touch_hw_io gpio = {
        (void *)touch_service_gpio, read32, write32, 0
    };
    const touch_hw_io clock = {
        (void *)touch_service_clock, read32, write32, delay_us
    };
    const touch_i2c_io i2c0 = {
        (void *)touch_service_i2c0, read32, write32
    };
    const touch_i2c_io i2c1 = {
        (void *)(touch_service_i2c0 + 0x1000), read32, write32
    };
    const touch_spi2_io spi = {
        (void *)touch_service_spi2, read32, write32
    };
    ServiceLink service = { .gpio = gpio, .spi = spi };
    const touch_boot_link boot = {
        &service, select_chip, boot_transfer, delay_us,
        wait_irq, boot_progress
    };
    const touch_z2_link reports = {
        &service, select_chip, spi_transfer, delay_us
    };
    touch_z2_result d9 = {0};
    touch_z2_frame packet = {0};
    IphoneTouchFrame contacts = {0};
    IphoneTouchCalibration calibration;
    uint8_t value = 0, before = 0, after = 0;
    uint32_t old_clock = 0, old_irq = 0, packets = 0, tag = 0, stage = 1;
    int powered = 0, clock_touched = 0, irq_configured = 0;

    ((volatile IphoneTouchState *)touch_service_shared_state)->revision = 0;
    ((volatile IphoneTouchState *)touch_service_shared_state)->event_write = 0;
    publish(IPHONE_TOUCH_STARTING, stage, 0, 0, 0, 0, 0);
    if (touch_hw_enable_gate(&pmgr, 0x1a8) != TOUCH_HW_OK ||
        touch_hw_enable_gate(&pmgr, 0x1b0) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 197) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 196) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 40) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 39) != TOUCH_HW_OK)
        goto fail;
    stage = 2;
    if (touch_i2c_pmu_read(&i2c1, 0x74, 0x031f, 2, 2000000, &value) !=
            TOUCH_I2C_OK || value != 1 ||
        touch_i2c_pmu_read(&i2c0, 0x27, 0x05, 1, 2000000, &value) !=
            TOUCH_I2C_OK || value != 0x0f)
        goto fail;
    old_clock = read32(clock.context, TOUCH_CLOCK_OFFSET);
    if (old_clock != 0x800002eeu ||
        touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 0) != TOUCH_HW_OK ||
        touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 1) != TOUCH_HW_OK)
        goto fail;
    stage = 3;
    if (touch_i2c_pmu_update(&i2c0, 0x27, 0x05, 1, 0x10, 0x10,
                             2000000, &before, &after) != TOUCH_I2C_OK ||
        after != 0x1fu)
        goto fail;
    powered = 1;
    delay_us(0, 3000);
    stage = 4;
    clock_touched = 1;
    if (touch_hw_enable_clock(&clock) != TOUCH_HW_OK) goto fail;
    delay_us(0, 1500);
    stage = 5;
    if (touch_hw_enable_gate(&pmgr, TOUCH_SPI2_GATE_OFFSET) != TOUCH_HW_OK ||
        touch_spi2_prepare(&spi, 24000000, 3000000) != TOUCH_SPI2_OK)
        goto fail;
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 1);
    delay_us(0, 200);
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 0);
    delay_us(0, 1500);
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 1);
    delay_us(0, 1000);
    if (sync_four(&service)) goto fail;
    stage = 6;
    old_irq = read32(gpio.context, TOUCH_IRQ_GPIO * 4u);
    if (touch_hw_gpio_irq_falling(&gpio, TOUCH_IRQ_GPIO) != TOUCH_HW_OK)
        goto fail;
    irq_configured = 1;
    touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 1);
    delay_us(0, 200);
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 0);
    delay_us(0, 1500);
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 1);
    (void)wait_irq(&service, 500);
    if (sync_four(&service)) goto fail;
    touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
    stage = 7;
    if (touch_boot_execute(touch_firmware_start,
                           (size_t)(touch_firmware_end - touch_firmware_start),
                           &boot) != TOUCH_BOOT_EXEC_OK)
        goto fail;
    delay_us(0, 50000);
    stage = 8;
    if (touch_z2_read_d9(&reports, &d9) != TOUCH_Z2_OK ||
        touch_z2_enable_reports(&reports) != TOUCH_Z2_OK)
        goto fail;
    calibration.left = (int16_t)((uint16_t)d9.d9[8] |
                                  ((uint16_t)d9.d9[9] << 8));
    calibration.bottom = (int16_t)((uint16_t)d9.d9[10] |
                                    ((uint16_t)d9.d9[11] << 8));
    calibration.right = (int16_t)((uint16_t)d9.d9[12] |
                                   ((uint16_t)d9.d9[13] << 8));
    calibration.top = (int16_t)((uint16_t)d9.d9[14] |
                                 ((uint16_t)d9.d9[15] << 8));
    touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
    publish(IPHONE_TOUCH_READY, stage, 0, 0, 0, 0, 0);
    uint64_t last_frame_wake = 0;
    for (;;) {
        if (slint_frame_wake_poll((const uint64_t *)touch_frame_wake_request, &last_frame_wake))
            microkit_notify(1);
        int pending = touch_hw_gpio_irq_pending(&gpio, TOUCH_IRQ_GPIO);
        uint32_t pin = read32(gpio.context, TOUCH_IRQ_GPIO * 4u);
        if (!pending && (pin & 1u)) {
            delay_us(0, 100);
            seL4_Yield();
            continue;
        }
        if (pending)
            touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
        if (touch_z2_read_frame(&reports, tag, &packet) != TOUCH_Z2_OK)
            continue;
        tag ^= 1u;
        packets++;
        if (packet.payload_length < 24 ||
            iphone_touch_decode_report(packet.data, packet.payload_length,
                                       &calibration, &contacts))
            continue;
        uint32_t active = 0, finger = 0, x = 0, y = 0;
        for (unsigned i = 0; i < contacts.count; i++) {
            if (contacts.contacts[i].active) {
                active = 1;
                finger = contacts.contacts[i].finger;
                x = contacts.contacts[i].x;
                y = contacts.contacts[i].y;
                break;
            }
        }
        publish(IPHONE_TOUCH_READY, stage, packets, active, finger, x, y);
    }

fail:
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 0);
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 0);
    if (irq_configured) {
        touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
        write32(gpio.context, TOUCH_IRQ_GPIO * 4u, old_irq);
    }
    if (clock_touched) {
        touch_hw_disable_clock(&clock);
        write32(clock.context, TOUCH_CLOCK_OFFSET, old_clock);
    }
    if (powered)
        touch_i2c_pmu_update(&i2c0, 0x27, 0x05, 1, 0x10, 0,
                             2000000, &before, &after);
    publish(IPHONE_TOUCH_ERROR, stage, packets, 0, 0, 0, 0);
}

void notified(microkit_channel channel) { (void)channel; }
