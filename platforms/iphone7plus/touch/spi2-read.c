/* D11 16 KiB Microkit touch hardware diagnostics. */
#include <microkit.h>
#include <stdint.h>
#ifdef SPI2_SETUP
#include "spi2_transport.h"
#endif
#ifdef TOUCH_HW_PROBE
#include "hw_control.h"
#endif
#ifdef TOUCH_PMU_PROBE
#include "hw_control.h"
#include "i2c_pmu.h"
#endif
#ifdef TOUCH_STARTUP_PROBE
#include "hw_control.h"
#include "i2c_pmu.h"
#include "spi2_transport.h"
#ifdef TOUCH_FIRMWARE_PROBE
#include "boot_execute.h"
#include "boot_stream.h"
#ifdef TOUCH_REPORT_PROBE
#include "z2_report.h"
#ifdef TOUCH_EVENT_PROBE
#include "report.h"
#endif
#endif
#endif
#endif

uintptr_t spi2_probe_framebuffer;
uintptr_t spi2_probe_mmio;
uintptr_t spi2_probe_gate;
volatile uint32_t spi2_probe_stage;
volatile uint32_t spi2_probe_values[5];
#if defined(TOUCH_HW_PROBE) || defined(TOUCH_PMU_PROBE) || defined(TOUCH_STARTUP_PROBE)
uintptr_t touch_probe_i2c0;
uintptr_t touch_probe_clock;
uintptr_t touch_probe_gpio;
#endif

enum { STRIDE = 0x1100 / 4, WIDTH = 640, HEIGHT = 240 };

static const char *glyph(char c)
{
    switch (c) {
    case '0': return "01110100011001110101110011000101110";
    case '1': return "00100011000010000100001000010001110";
    case '2': return "01110100010000100010001000100011111";
    case '3': return "11110000010000101110000010000111110";
    case '4': return "00010001100101010010111110001000010";
    case '5': return "11111100001000011110000010000111110";
    case '6': return "01110100001000011110100011000101110";
    case '7': return "11111000010001000100010000100001000";
    case '8': return "01110100011000101110100011000101110";
    case '9': return "01110100011000101111000010000101110";
    case 'A': return "01110100011000111111100011000110001";
    case 'B': return "11110100011000111110100011000111110";
    case 'C': return "01111100001000010000100001000001111";
    case 'D': return "11110100011000110001100011000111110";
    case 'E': return "11111100001000011110100001000011111";
    case 'F': return "11111100001000011110100001000010000";
    case 'G': return "01111100001000010111100011000101111";
    case 'H': return "10001100011000111111100011000110001";
    case 'I': return "11111001000010000100001000010011111";
    case 'K': return "10001100101010011000101001001010001";
    case 'L': return "10000100001000010000100001000011111";
    case 'M': return "10001110111010110101100011000110001";
    case 'N': return "10001110011010110011100011000110001";
    case 'O': return "01110100011000110001100011000101110";
    case 'P': return "11110100011000111110100001000010000";
    case 'R': return "11110100011000111110101001001010001";
    case 'S': return "01111100001000001110000010000111110";
    case 'T': return "11111001000010000100001000010000100";
    case 'U': return "10001100011000110001100011000101110";
    case 'V': return "10001100011000110001010100010000100";
    case 'W': return "10001100011010110101101011010101010";
    default: return "00000000000000000000000000000000000";
    }
}

static void draw_char(unsigned x, unsigned y, char c, uint32_t color)
{
    volatile uint32_t *fb = (volatile uint32_t *)spi2_probe_framebuffer;
    const char *bitmap = glyph(c);
    for (unsigned row = 0; row < 7; row++)
        for (unsigned col = 0; col < 5; col++)
            if (bitmap[row * 5 + col] == '1')
                for (unsigned dy = 0; dy < 3; dy++)
                    for (unsigned dx = 0; dx < 3; dx++)
                        fb[(y + row * 3 + dy) * STRIDE + x + col * 3 + dx] = color;
}

static void draw_text(unsigned x, unsigned y, const char *value, uint32_t color)
{
    for (unsigned i = 0; value[i] && i < 30; i++)
        draw_char(x + i * 18, y, value[i], color);
}

static void draw_hex(unsigned x, unsigned y, uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    char output[9];
    for (unsigned i = 0; i < 8; i++)
        output[i] = digits[(value >> (28 - i * 4)) & 15];
    output[8] = 0;
    draw_text(x, y, output, 0xffffff00u);
}

static void draw_hex64(unsigned x, unsigned y, uint64_t value)
{
    draw_hex(x, y, (uint32_t)(value >> 32));
    draw_hex(x + 8 * 18, y, (uint32_t)value);
}

static void clear_screen(void)
{
    volatile uint32_t *fb = (volatile uint32_t *)spi2_probe_framebuffer;
    for (unsigned y = 0; y < HEIGHT; y++)
        for (unsigned x = 0; x < WIDTH; x++)
            fb[y * STRIDE + x] = 0xff101820u;
}

#if defined(TOUCH_HW_PROBE) || defined(TOUCH_PMU_PROBE) || defined(TOUCH_STARTUP_PROBE)
static uint32_t touch_read_register(void *context, uint32_t offset)
{
    volatile uint32_t *base = context;
    return base[offset / 4];
}

static void touch_write_register(void *context, uint32_t offset, uint32_t value)
{
    volatile uint32_t *base = context;
    base[offset / 4] = value;
    asm volatile("dsb sy" ::: "memory");
}
#endif

#ifdef TOUCH_HW_PROBE
void init(void)
{
    const touch_hw_io pmgr = {
        (void *)spi2_probe_gate, touch_read_register, touch_write_register, 0
    };
    volatile const uint32_t *i2c0 = (volatile const uint32_t *)touch_probe_i2c0;
    volatile const uint32_t *i2c1 = (volatile const uint32_t *)(touch_probe_i2c0 + 0x1000);
    volatile const uint32_t *clock = (volatile const uint32_t *)touch_probe_clock;
    volatile const uint32_t *gpio = (volatile const uint32_t *)touch_probe_gpio;

    clear_screen();
    draw_text(20, 12, "HW PROBE", 0xffffffffu);
    draw_text(190, 12, "PTR", 0xffffffffu);
    draw_hex64(250, 12, (uint64_t)touch_probe_i2c0);
    draw_text(20, 48, "I2C0 GATE", 0xffffffffu);
    spi2_probe_stage = 1;
    if (touch_hw_enable_gate(&pmgr, 0x1a8) != TOUCH_HW_OK) {
        draw_text(230, 48, "FAIL", 0xffff0000u);
        return;
    }
    draw_hex(230, 48, touch_read_register(pmgr.context, 0x1a8));
    draw_text(20, 72, "I2C0 REV", 0xffffffffu);
    spi2_probe_stage = 2;
    draw_hex(230, 72, i2c0[0x28 / 4]);

    draw_text(20, 96, "I2C1 GATE", 0xffffffffu);
    spi2_probe_stage = 3;
    if (touch_hw_enable_gate(&pmgr, 0x1b0) != TOUCH_HW_OK) {
        draw_text(230, 96, "FAIL", 0xffff0000u);
        return;
    }
    draw_hex(230, 96, touch_read_register(pmgr.context, 0x1b0));
    draw_text(20, 120, "I2C1 REV", 0xffffffffu);
    spi2_probe_stage = 4;
    draw_hex(230, 120, i2c1[0x28 / 4]);

    draw_text(20, 144, "TOUCH CLK", 0xffffffffu);
    spi2_probe_stage = 5;
    draw_hex(230, 144, clock[0]);
    draw_text(20, 168, "RESET GPIO", 0xffffffffu);
    spi2_probe_stage = 6;
    draw_hex(230, 168, gpio[TOUCH_RESET_GPIO]);
    draw_text(20, 192, "CS GPIO", 0xffffffffu);
    spi2_probe_stage = 7;
    draw_hex(230, 192, gpio[TOUCH_CS_GPIO]);
    draw_text(20, 216, "IRQ GPIO", 0xffffffffu);
    spi2_probe_stage = 8;
    draw_hex(230, 216, gpio[TOUCH_IRQ_GPIO]);
    asm volatile("dsb sy" ::: "memory");
}
#elif defined(TOUCH_PMU_PROBE)
void init(void)
{
    const touch_hw_io pmgr = {
        (void *)spi2_probe_gate, touch_read_register, touch_write_register, 0
    };
    const touch_hw_io gpio = {
        (void *)touch_probe_gpio, touch_read_register, touch_write_register, 0
    };
    const touch_i2c_io i2c0 = {
        (void *)touch_probe_i2c0, touch_read_register, touch_write_register
    };
    const touch_i2c_io i2c1 = {
        (void *)(touch_probe_i2c0 + 0x1000),
        touch_read_register, touch_write_register
    };
    uint8_t value;
#ifdef TOUCH_HV_PROBE
    uint8_t before, after;
    int on_result;
#endif
    int result;
    clear_screen();
#ifdef TOUCH_HV_PROBE
    draw_text(20, 12, "TOUCH HV", 0xffffffffu);
#else
    draw_text(20, 12, "PMU READ", 0xffffffffu);
#endif
    draw_text(190, 12, "PTR", 0xffffffffu);
    draw_hex64(250, 12, (uint64_t)touch_probe_i2c0);

    draw_text(20, 48, "GATE0", 0xffffffffu);
    spi2_probe_stage = 1;
    if (touch_hw_enable_gate(&pmgr, 0x1a8) != TOUCH_HW_OK) {
        draw_text(230, 48, "FAIL", 0xffff0000u);
        return;
    }
    draw_hex(230, 48, touch_read_register(pmgr.context, 0x1a8));
    draw_text(20, 72, "GATE1", 0xffffffffu);
    spi2_probe_stage = 2;
    if (touch_hw_enable_gate(&pmgr, 0x1b0) != TOUCH_HW_OK) {
        draw_text(230, 72, "FAIL", 0xffff0000u);
        return;
    }
    draw_hex(230, 72, touch_read_register(pmgr.context, 0x1b0));

    draw_text(20, 96, "I2C PINS", 0xffffffffu);
    spi2_probe_stage = 3;
    if (touch_hw_gpio_peripheral(&gpio, 197) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 196) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 40) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 39) != TOUCH_HW_OK) {
        draw_text(230, 96, "FAIL", 0xffff0000u);
        return;
    }
    draw_text(230, 96, "OK", 0xff00ff00u);

#ifdef TOUCH_HV_PROBE
    draw_text(20, 120, "CORE 31F", 0xffffffffu);
    spi2_probe_stage = 4;
    result = touch_i2c_pmu_read(&i2c1, 0x74, 0x031f, 2, 2000000, &value);
    if (result != TOUCH_I2C_OK) {
        draw_text(230, 120, "ERR", 0xffff0000u);
        draw_hex(310, 120, (uint32_t)result);
        return;
    }
    draw_hex(230, 120, value);
    if (!(value & 1u)) {
        draw_text(430, 120, "STOP", 0xffff0000u);
        return;
    }

    draw_text(20, 144, "HV BEFORE", 0xffffffffu);
    spi2_probe_stage = 5;
    result = touch_i2c_pmu_read(&i2c0, 0x27, 0x05, 1, 2000000, &value);
    if (result != TOUCH_I2C_OK) {
        draw_text(230, 144, "ERR", 0xffff0000u);
        draw_hex(310, 144, (uint32_t)result);
        return;
    }
    draw_hex(230, 144, value);
    if (value != 0x0fu) {
        draw_text(430, 144, "STOP", 0xffff0000u);
        return;
    }

    spi2_probe_stage = 6;
    if (touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 0) != TOUCH_HW_OK) {
        draw_text(430, 144, "RESET FAIL", 0xffff0000u);
        return;
    }
    draw_text(20, 168, "HV ON", 0xffffffffu);
    spi2_probe_stage = 7;
    on_result = touch_i2c_pmu_update(&i2c0, 0x27, 0x05, 1, 0x10, 0x10,
                                     2000000, &before, &after);
    if (on_result == TOUCH_I2C_OK)
        draw_hex(230, 168, after);
    else {
        draw_text(230, 168, "ERR", 0xffff0000u);
        draw_hex(310, 168, (uint32_t)on_result);
    }

    /* Attempt to restore the original rail state even after a failed write
     * or readback: the first transaction may have changed the PMU. */
    draw_text(20, 192, "HV OFF", 0xffffffffu);
    spi2_probe_stage = 8;
    result = touch_i2c_pmu_update(&i2c0, 0x27, 0x05, 1, 0x10, 0x00,
                                  2000000, &before, &after);
    if (result != TOUCH_I2C_OK) {
        draw_text(230, 192, "ERR", 0xffff0000u);
        draw_hex(310, 192, (uint32_t)result);
        return;
    }
    draw_hex(230, 192, after);
    if (after != 0x0fu) {
        draw_text(430, 192, "FAIL", 0xffff0000u);
        return;
    }
    if (on_result != TOUCH_I2C_OK || before != 0x1fu) {
        draw_text(20, 216, "ON FAIL", 0xffff0000u);
        return;
    }
    draw_text(20, 216, "DONE", 0xff00ff00u);
    spi2_probe_stage = 9;
#else
    draw_text(20, 120, "I2C0 REV", 0xffffffffu);
    spi2_probe_stage = 4;
    draw_hex(230, 120, touch_read_register(i2c0.context, 0x28));
    draw_text(20, 144, "I2C1 REV", 0xffffffffu);
    spi2_probe_stage = 5;
    draw_hex(230, 144, touch_read_register(i2c1.context, 0x28));

    draw_text(20, 168, "HV REG5", 0xffffffffu);
    spi2_probe_stage = 6;
    result = touch_i2c_pmu_read(&i2c0, 0x27, 0x05, 1, 2000000, &value);
    if (result != TOUCH_I2C_OK) {
        draw_text(230, 168, "ERR", 0xffff0000u);
        draw_hex(310, 168, (uint32_t)result);
        return;
    }
    draw_hex(230, 168, value);

    draw_text(20, 192, "CORE 31F", 0xffffffffu);
    spi2_probe_stage = 7;
    result = touch_i2c_pmu_read(&i2c1, 0x74, 0x031f, 2, 2000000, &value);
    if (result != TOUCH_I2C_OK) {
        draw_text(230, 192, "ERR", 0xffff0000u);
        draw_hex(310, 192, (uint32_t)result);
        return;
    }
    draw_hex(230, 192, value);
    draw_text(20, 216, "DONE", 0xff00ff00u);
    spi2_probe_stage = 8;
#endif
    asm volatile("dsb sy" ::: "memory");
}
#elif defined(TOUCH_STARTUP_PROBE)
static void touch_delay_us(void *context, unsigned microseconds)
{
    uint32_t iterations;
    (void)context;
    if (!microseconds)
        return;
    /* The current seL4 build traps EL0 reads of CNTVCT_EL0. A conservative
     * instruction loop supplies the short sequencing gaps without touching
     * a privileged timer register. These are minimum waits, not a clock. */
    iterations = microseconds * 4000u;
    asm volatile("1: subs %w0, %w0, #1\n"
                 "   b.ne 1b\n"
                 : "+r"(iterations) :: "cc");
}

#ifdef TOUCH_FIRMWARE_PROBE
extern const uint8_t touch_firmware_start[];
extern const uint8_t touch_firmware_end[];

typedef struct {
    touch_hw_io gpio;
    touch_spi2_io spi;
    uint32_t irq_reg;
    uint32_t irq_pending;
    uint32_t irq_seen;
    uint32_t irq_first_record;
    uint32_t irq_before_wait;
    uint32_t irq_after_ack;
    int spi_error;
} touch_firmware_context;

static void clear_row(unsigned y)
{
    volatile uint32_t *fb = (volatile uint32_t *)spi2_probe_framebuffer;
    for (unsigned row = y; row < y + 21; row++)
        for (unsigned x = 0; x < WIDTH; x++)
            fb[row * STRIDE + x] = 0xff101820u;
}

static int firmware_chip_select(void *context, int asserted)
{
    touch_firmware_context *firmware = context;
    return touch_hw_gpio_output(&firmware->gpio, TOUCH_CS_GPIO, !asserted);
}

static int firmware_transfer(void *context, const uint8_t *bytes, size_t length)
{
    touch_firmware_context *firmware = context;
    /* Each Sandcastle write is a separate spi_sync_transfer, which calls
     * hx_spi_prepare even when the external touch CS stays asserted across
     * consecutive 16 KiB chunks of the same firmware record. */
    firmware->spi_error = touch_spi2_prepare(&firmware->spi,
                                             24000000, 3000000);
    if (firmware->spi_error != TOUCH_SPI2_OK)
        return 1;
    firmware->spi_error = touch_spi2_transfer(&firmware->spi, bytes, 0,
                                              length, 5000000);
    return firmware->spi_error != TOUCH_SPI2_OK;
}

#ifdef TOUCH_REPORT_PROBE
static int report_transfer(void *context, const uint8_t *tx, uint8_t *rx,
                           size_t length)
{
    touch_firmware_context *firmware = context;
    /* Sandcastle's hx_spi_transfer_one_message calls hx_spi_prepare for
     * each spi_sync_transfer, including each generation-2 control command. */
    firmware->spi_error = touch_spi2_prepare(&firmware->spi,
                                              24000000, 3000000);
    if (firmware->spi_error != TOUCH_SPI2_OK)
        return 1;
    firmware->spi_error = touch_spi2_transfer(&firmware->spi, tx, rx,
                                              length, 5000000);
    return firmware->spi_error != TOUCH_SPI2_OK;
}

static uint32_t d9_word(const uint8_t *bytes)
{
    return (uint32_t)bytes[0] | ((uint32_t)bytes[1] << 8);
}

static uint32_t d9_four(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24) |
           ((uint32_t)bytes[1] << 16) |
           ((uint32_t)bytes[2] << 8) | bytes[3];
}
#endif

static void firmware_delay(void *context, unsigned microseconds)
{
    (void)context;
    touch_delay_us(0, microseconds);
}

static int firmware_wait_irq(void *context, unsigned timeout_ms)
{
    touch_firmware_context *firmware = context;
    for (unsigned elapsed = 0; elapsed <= timeout_ms; elapsed++) {
        firmware->irq_pending = (uint32_t)touch_hw_gpio_irq_pending(
            &firmware->gpio, TOUCH_IRQ_GPIO);
        firmware->irq_reg = touch_read_register(firmware->gpio.context,
                                                TOUCH_IRQ_GPIO * 4u);
        if (firmware->irq_pending || !(firmware->irq_reg & 1u)) {
            firmware->irq_seen = firmware->irq_pending ? 1u : 2u;
            if (firmware->irq_pending)
                touch_hw_gpio_irq_ack(&firmware->gpio, TOUCH_IRQ_GPIO);
            return 0;
        }
        touch_delay_us(0, 1000);
    }
    return -1;
}

static void firmware_progress(void *context, uint32_t index, uint32_t type)
{
    touch_firmware_context *firmware = context;
    if (touch_hw_gpio_irq_pending(&firmware->gpio, TOUCH_IRQ_GPIO) &&
        firmware->irq_first_record == 0xffffffffu)
        firmware->irq_first_record = index;
    if (type == TOUCH_BOOT_WAIT_IRQ)
        firmware->irq_before_wait = (uint32_t)touch_hw_gpio_irq_pending(
            &firmware->gpio, TOUCH_IRQ_GPIO);
    clear_row(120);
    clear_row(144);
    draw_text(20, 120, "RECORD", 0xffffffffu);
    draw_hex(260, 120, index);
    draw_text(20, 144, "TYPE", 0xffffffffu);
    draw_hex(260, 144, type);
}
#endif

void init(void)
{
    const touch_hw_io pmgr = {
        (void *)spi2_probe_gate, touch_read_register, touch_write_register, 0
    };
    const touch_hw_io gpio = {
        (void *)touch_probe_gpio, touch_read_register, touch_write_register, 0
    };
    const touch_hw_io clock = {
        (void *)touch_probe_clock, touch_read_register, touch_write_register,
        touch_delay_us
    };
    const touch_i2c_io i2c0 = {
        (void *)touch_probe_i2c0, touch_read_register, touch_write_register
    };
    const touch_i2c_io i2c1 = {
        (void *)(touch_probe_i2c0 + 0x1000),
        touch_read_register, touch_write_register
    };
    const touch_spi2_io spi = {
        (void *)spi2_probe_mmio, touch_read_register, touch_write_register
    };
    const uint8_t tx[4] = { 0, 0, 0, 0 };
    uint8_t rx[4] = { 0, 0, 0, 0 };
    uint8_t value, before, after;
    uint32_t old_clock, received;
    int result, success = 0, clock_touched = 0, clock_restored = 1;
#ifdef TOUCH_FIRMWARE_PROBE
    touch_firmware_context firmware = {
        .gpio = gpio, .spi = spi, .irq_first_record = 0xffffffffu
    };
    uint32_t old_irq_gpio = 0;
    int irq_configured = 0;
    const touch_boot_link boot_link = {
        &firmware, firmware_chip_select, firmware_transfer,
        firmware_delay, firmware_wait_irq, firmware_progress
    };
#ifdef TOUCH_REPORT_PROBE
    const touch_z2_link report_link = {
        &firmware, firmware_chip_select, report_transfer, firmware_delay
    };
    touch_z2_result d9 = {0};
#ifndef TOUCH_EVENT_PROBE
    touch_z2_result d9_late = {0};
    uint32_t late_irq_pending = 0;
    int d9_late_error = TOUCH_Z2_INVALID;
#endif
    int d9_attempted = 0, d9_error = TOUCH_Z2_INVALID;
#ifdef TOUCH_EVENT_PROBE
    touch_z2_frame event = {0};
    IphoneTouchFrame contacts = {0};
    uint32_t event_irqs = 0, event_tag = 0;
    uint32_t event_line_before = 0, event_line_after = 0;
    int event_error = TOUCH_Z2_INVALID, event_decode = -1;
#endif
#endif
#endif

    clear_screen();
    draw_text(20, 12, "TOUCH START", 0xffffffffu);
    draw_text(230, 12, "SPI", 0xffffffffu);
    draw_hex64(290, 12, (uint64_t)spi2_probe_mmio);
    spi2_probe_stage = 1;
    if (touch_hw_enable_gate(&pmgr, 0x1a8) != TOUCH_HW_OK ||
        touch_hw_enable_gate(&pmgr, 0x1b0) != TOUCH_HW_OK) {
        draw_text(20, 48, "I2C GATE FAIL", 0xffff0000u);
        return;
    }
    if (touch_hw_gpio_peripheral(&gpio, 197) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 196) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 40) != TOUCH_HW_OK ||
        touch_hw_gpio_peripheral(&gpio, 39) != TOUCH_HW_OK) {
        draw_text(20, 48, "I2C PINS FAIL", 0xffff0000u);
        return;
    }

    draw_text(20, 48, "CORE", 0xffffffffu);
    spi2_probe_stage = 2;
    result = touch_i2c_pmu_read(&i2c1, 0x74, 0x031f, 2, 2000000, &value);
    if (result != TOUCH_I2C_OK || value != 1) {
        draw_text(260, 48, "STOP", 0xffff0000u);
        draw_hex(360, 48, (uint32_t)result);
        return;
    }
    draw_hex(260, 48, value);

    draw_text(20, 72, "HV BEFORE", 0xffffffffu);
    spi2_probe_stage = 3;
    result = touch_i2c_pmu_read(&i2c0, 0x27, 0x05, 1, 2000000, &value);
    if (result != TOUCH_I2C_OK || value != 0x0f) {
        draw_text(260, 72, "STOP", 0xffff0000u);
        draw_hex(360, 72, (uint32_t)result);
        return;
    }
    draw_hex(260, 72, value);
    old_clock = touch_read_register(clock.context, TOUCH_CLOCK_OFFSET);
    if (old_clock != 0x800002eeu) {
        draw_text(20, 96, "CLOCK STATE STOP", 0xffff0000u);
        draw_hex(360, 96, old_clock);
        return;
    }
    if (touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 0) != TOUCH_HW_OK ||
        touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 1) != TOUCH_HW_OK) {
        draw_text(20, 96, "GPIO FAIL", 0xffff0000u);
        return;
    }

    draw_text(20, 96, "HV ON", 0xffffffffu);
    spi2_probe_stage = 4;
    result = touch_i2c_pmu_update(&i2c0, 0x27, 0x05, 1, 0x10, 0x10,
                                  2000000, &before, &after);
    if (result != TOUCH_I2C_OK || after != 0x1fu) {
        draw_text(260, 96, "ERR", 0xffff0000u);
        draw_hex(360, 96, (uint32_t)result);
        goto cleanup;
    }
    draw_hex(260, 96, after);
    touch_delay_us(0, 3000);

    draw_text(20, 120, "CLOCK", 0xffffffffu);
    spi2_probe_stage = 5;
    clock_touched = 1;
    result = touch_hw_enable_clock(&clock);
    if (result != TOUCH_HW_OK) {
        draw_text(260, 120, "ERR", 0xffff0000u);
        draw_hex(360, 120, (uint32_t)result);
        goto cleanup;
    }
    draw_hex(260, 120, touch_read_register(clock.context, TOUCH_CLOCK_OFFSET));
    touch_delay_us(0, 1500);

    draw_text(20, 144, "SPI2", 0xffffffffu);
    spi2_probe_stage = 6;
    if (touch_hw_enable_gate(&pmgr, TOUCH_SPI2_GATE_OFFSET) != TOUCH_HW_OK) {
        draw_text(260, 144, "GATE FAIL", 0xffff0000u);
        goto cleanup;
    }
    result = touch_spi2_prepare(&spi, 24000000, 3000000);
    if (result != TOUCH_SPI2_OK) {
        draw_text(260, 144, "ERR", 0xffff0000u);
        draw_hex(360, 144, (uint32_t)result);
        goto cleanup;
    }
    draw_text(260, 144, "READY", 0xff00ff00u);

    spi2_probe_stage = 7;
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 1);
    touch_delay_us(0, 200);
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 0);
    touch_delay_us(0, 1500);
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 1);
    touch_delay_us(0, 1000);
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 0);
    draw_text(20, 168, "RX FOUR", 0xffffffffu);
    spi2_probe_stage = 8;
    result = touch_spi2_prepare(&spi, 24000000, 3000000);
    if (result == TOUCH_SPI2_OK)
        result = touch_spi2_transfer(&spi, tx, rx, sizeof(tx), 2000000);
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 1);
    if (result != TOUCH_SPI2_OK) {
        draw_text(260, 168, "ERR", 0xffff0000u);
        draw_hex(360, 168, (uint32_t)result);
        goto cleanup;
    }
    received = ((uint32_t)rx[0] << 24) | ((uint32_t)rx[1] << 16) |
               ((uint32_t)rx[2] << 8) | rx[3];
    draw_hex(260, 168, received);
#ifdef TOUCH_FIRMWARE_PROBE
    clear_screen();
    draw_text(20, 12, "D11 FIRMWARE", 0xffffffffu);
    draw_text(20, 48, "PROBE RX", 0xffffffffu);
    draw_hex(260, 48, received);

    /* The daemon repeats reset, waits for boot IRQ, then clocks four sync
     * bytes with CS asserted before it sends the personality stream. */
    spi2_probe_stage = 9;
    old_irq_gpio = touch_read_register(gpio.context, TOUCH_IRQ_GPIO * 4u);
    if (touch_hw_gpio_irq_falling(&gpio, TOUCH_IRQ_GPIO) != TOUCH_HW_OK) {
        draw_text(20, 72, "IRQ CONFIG FAIL", 0xffff0000u);
        goto cleanup;
    }
    irq_configured = 1;
    touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 1);
    touch_delay_us(0, 200);
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 0);
    touch_delay_us(0, 1500);
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 1);
    draw_text(20, 72, "BOOT IRQ", 0xffffffffu);
    result = firmware_wait_irq(&firmware, 500);
    draw_hex(260, 72, firmware.irq_reg);
    if (result)
        draw_text(430, 72, "TIMEOUT", 0xffff0000u);
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 0);
    touch_delay_us(0, 1000);
    result = touch_spi2_prepare(&spi, 24000000, 3000000);
    if (result == TOUCH_SPI2_OK)
        result = touch_spi2_transfer(&spi, tx, rx, sizeof(tx), 2000000);
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 1);
    draw_text(20, 96, "SYNC RX", 0xffffffffu);
    if (result != TOUCH_SPI2_OK) {
        draw_text(260, 96, "ERR", 0xffff0000u);
        draw_hex(360, 96, (uint32_t)result);
        goto cleanup;
    }
    received = ((uint32_t)rx[0] << 24) | ((uint32_t)rx[1] << 16) |
               ((uint32_t)rx[2] << 8) | rx[3];
    draw_hex(260, 96, received);

    /* The boot indication is separate from the firmware-complete edge. */
    touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
    firmware.irq_after_ack = (uint32_t)touch_hw_gpio_irq_pending(
        &gpio, TOUCH_IRQ_GPIO);
    firmware.irq_seen = 0;

    spi2_probe_stage = 10;
    result = touch_boot_execute(touch_firmware_start,
                                (size_t)(touch_firmware_end - touch_firmware_start),
                                &boot_link);
    draw_text(20, 168, "FW RESULT", 0xffffffffu);
    draw_hex(260, 168, (uint32_t)result);
    if (result == TOUCH_BOOT_EXEC_IO)
        draw_hex(430, 168, (uint32_t)firmware.spi_error);
    draw_text(390, 192, "IRQ", 0xffffffffu);
    draw_hex(450, 192, firmware.irq_seen);
    draw_hex(450, 216, firmware.irq_reg);
    if (result == TOUCH_BOOT_EXEC_OK) {
        touch_delay_us(0, 50000);
        success = 1;
#ifdef TOUCH_REPORT_PROBE
        d9_attempted = 1;
        d9_error = touch_z2_read_d9(&report_link, &d9);
#ifdef TOUCH_EVENT_PROBE
        if (d9_error == TOUCH_Z2_OK) {
            IphoneTouchCalibration calibration = {
                (int16_t)d9_word(d9.d9 + 8),
                (int16_t)d9_word(d9.d9 + 12),
                (int16_t)d9_word(d9.d9 + 14),
                (int16_t)d9_word(d9.d9 + 10)
            };
            event_error = touch_z2_enable_reports(&report_link);
            if (event_error == TOUCH_Z2_OK) {
                event_error = TOUCH_Z2_NO_EVENT;
                touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
                event_line_before = !(touch_read_register(
                    gpio.context, TOUCH_IRQ_GPIO * 4u) & 1u);
                clear_screen();
                draw_text(20, 12, "D11 TOUCH NOW", 0xffffffffu);
                draw_text(20, 48, "TAP SCREEN", 0xffffff00u);
                draw_text(20, 96, "WAIT", 0xffffffffu);
                for (unsigned elapsed = 0; elapsed < 3000; elapsed++) {
                    int pending, line_low;
                    if (elapsed % 200 == 0) {
                        clear_row(120);
                        draw_hex(260, 120, elapsed);
                    }
                    pending = touch_hw_gpio_irq_pending(&gpio, TOUCH_IRQ_GPIO);
                    line_low = !(touch_read_register(
                        gpio.context, TOUCH_IRQ_GPIO * 4u) & 1u);
                    if (!pending && !line_low) {
                        touch_delay_us(0, 1000);
                        continue;
                    }
                    if (pending)
                        touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
                    event_irqs++;
                    event_error = touch_z2_read_frame(&report_link,
                                                      event_tag, &event);
                    if (event_error != TOUCH_Z2_OK) break;
                    event_tag ^= 1u;
                    if (event.payload_length < 24) {
                        /* Generation-2 startup/status packets are shorter
                         * than a contact report. Keep waiting for a finger. */
                        event_error = TOUCH_Z2_NO_EVENT;
                        continue;
                    }
                    event_decode = iphone_touch_decode_report(
                        event.data, event.payload_length, &calibration,
                        &contacts);
                    if (event_decode != 0) {
                        event_error = TOUCH_Z2_PACKET;
                        break;
                    }
                    for (unsigned i = 0; i < contacts.count; i++)
                        if (contacts.contacts[i].active)
                            goto event_done;
                    event_error = TOUCH_Z2_NO_EVENT;
                    if (event_irqs >= 16) break;
                }
event_done:;
                event_line_after = !(touch_read_register(
                    gpio.context, TOUCH_IRQ_GPIO * 4u) & 1u);
            }
        }
#else
        if (d9_error != TOUCH_Z2_OK) {
            /* Keep the panel powered between reads so a delayed transition
             * can be distinguished from a permanently invalid reply. */
            touch_delay_us(0, 500000);
            late_irq_pending = (uint32_t)touch_hw_gpio_irq_pending(
                &gpio, TOUCH_IRQ_GPIO);
            d9_late_error = touch_z2_read_d9(&report_link, &d9_late);
        }
#endif
#endif
    }
#else
    success = 1;
#endif

cleanup:
    spi2_probe_stage = 9;
#ifdef TOUCH_FIRMWARE_PROBE
    if (irq_configured) {
        touch_hw_gpio_irq_ack(&gpio, TOUCH_IRQ_GPIO);
        touch_write_register(gpio.context, TOUCH_IRQ_GPIO * 4u,
                             old_irq_gpio);
    }
#endif
    touch_hw_gpio_output(&gpio, TOUCH_RESET_GPIO, 0);
    touch_hw_gpio_output(&gpio, TOUCH_CS_GPIO, 0);
    if (clock_touched) {
        if (touch_hw_disable_clock(&clock) != TOUCH_HW_OK)
            clock_restored = 0;
        touch_write_register(clock.context, TOUCH_CLOCK_OFFSET, old_clock);
        if (touch_read_register(clock.context, TOUCH_CLOCK_OFFSET) != old_clock)
            clock_restored = 0;
    }
    draw_text(20, 192, "HV OFF", 0xffffffffu);
    result = touch_i2c_pmu_update(&i2c0, 0x27, 0x05, 1, 0x10, 0,
                                  2000000, &before, &after);
    if (result != TOUCH_I2C_OK || after != 0x0fu) {
        draw_text(260, 192, "ERR", 0xffff0000u);
        draw_hex(360, 192, (uint32_t)result);
        return;
    }
    draw_hex(260, 192, after);
    if (!clock_restored) {
        draw_text(20, 216, "CLOCK OFF FAIL", 0xffff0000u);
        return;
    }
    draw_text(20, 216,
#ifdef TOUCH_FIRMWARE_PROBE
              success ? "STREAM SENT" : "FAILED",
#else
              success ? "DONE" : "FAILED",
#endif
              success ? 0xff00ff00u : 0xffff0000u);
#ifdef TOUCH_REPORT_PROBE
    if (d9_attempted) {
        clear_screen();
#ifdef TOUCH_EVENT_PROBE
        draw_text(20, 12, "D11 TOUCH EVENT", 0xffffffffu);
        draw_text(20, 48, "D9 RESULT", 0xffffffffu);
        draw_hex(260, 48, (uint32_t)d9_error);
        draw_text(20, 72, "EVENT RESULT", 0xffffffffu);
        draw_hex(260, 72, (uint32_t)event_error);
        draw_text(20, 96, "IRQ LOW", 0xffffffffu);
        draw_hex(170, 96, event_irqs);
        draw_hex(360, 96, event_line_before);
        draw_hex(490, 96, event_line_after);
        draw_text(20, 120, "HEADER", 0xffffffffu);
        draw_hex(260, 120, d9_four(event.header));
        draw_text(20, 144, "WIRE LENGTH", 0xffffffffu);
        draw_hex(260, 144, event.wire_length);
        draw_text(20, 168, "CONTACTS", 0xffffffffu);
        draw_hex(260, 168, contacts.count);
        draw_text(20, 192, "X Y", 0xffffffffu);
        draw_hex(170, 192, contacts.contacts[0].x);
        draw_hex(350, 192, contacts.contacts[0].y);
        draw_text(20, 216, "HV OFF", 0xffffffffu);
        draw_hex(260, 216, after);
#else
        draw_text(20, 12, "D11 D9 REPORT", 0xffffffffu);
        draw_text(20, 48, "FW IRQ", 0xffffffffu);
        draw_hex(260, 48, firmware.irq_seen);
        draw_text(20, 72, "D9 RESULT", 0xffffffffu);
        draw_hex(260, 72, (uint32_t)d9_error);
        if (d9_error == TOUCH_Z2_OK) {
            draw_text(20, 96, "D9 STAGE", 0xffffffffu);
            draw_hex(260, 96, d9.stage);
            draw_text(20, 120, "D9 STATUS", 0xffffffffu);
            draw_hex(260, 120, d9.controller_status);
            draw_text(20, 144, "D9 LENGTH", 0xffffffffu);
            draw_hex(260, 144, d9.controller_length);
        } else {
            draw_text(20, 96, "D9 LATE", 0xffffffffu);
            draw_hex(260, 96, (uint32_t)d9_late_error);
            draw_text(20, 120, "LATE IRQ", 0xffffffffu);
            draw_hex(260, 120, late_irq_pending);
            draw_text(20, 144, "LATE STATUS LEN", 0xffffffffu);
            draw_hex(260, 144, d9_late.controller_status);
            draw_hex(430, 144, d9_late.controller_length);
        }
        if (d9_error == TOUCH_Z2_OK) {
            draw_text(20, 168, "LEFT", 0xffffffffu);
            draw_hex(170, 168, d9_word(d9.d9 + 8));
            draw_text(350, 168, "RIGHT", 0xffffffffu);
            draw_hex(450, 168, d9_word(d9.d9 + 12));
            draw_text(20, 192, "TOP", 0xffffffffu);
            draw_hex(170, 192, d9_word(d9.d9 + 14));
            draw_text(350, 192, "BOTTOM", 0xffffffffu);
            draw_hex(475, 192, d9_word(d9.d9 + 10));
        } else {
            draw_text(20, 168, "INFO NOW", 0xffffffffu);
            draw_hex(170, 168, d9_four(d9.info_response));
            draw_hex(420, 168, d9_four(d9.info_response + 4));
            draw_text(20, 192, "INFO LATE", 0xffffffffu);
            draw_hex(170, 192, d9_four(d9_late.info_response));
            draw_hex(420, 192, d9_four(d9_late.info_response + 4));
            draw_text(20, 216, "WAKE LATE", 0xffffffffu);
            draw_hex(170, 216, d9_four(d9_late.wake_response));
            draw_hex(420, 216, d9_four(d9_late.wake_response + 4));
        }
        if (d9_error == TOUCH_Z2_OK) {
            draw_text(20, 216, "HV OFF", 0xffffffffu);
            draw_hex(260, 216, after);
        }
#endif
    }
#endif
    spi2_probe_stage = 10;
    asm volatile("dsb sy" ::: "memory");
}
#else

#ifdef SPI2_SETUP
static uint32_t spi2_read_register(void *context, uint32_t offset)
{
    volatile uint32_t *base = context;
    return base[offset / 4];
}

static void spi2_write_register(void *context, uint32_t offset, uint32_t value)
{
    volatile uint32_t *base = context;
    base[offset / 4] = value;
    asm volatile("dsb sy" ::: "memory");
}
#endif

void init(void)
{
    static const uint32_t offsets[] = { 0x00, 0x04, 0x08, 0x0c, 0x30 };
    static const char *labels[] = { "CLKCFG", "CONFIG", "STATUS", "PIN", "CLKDIV" };
    volatile const uint32_t *spi = (volatile const uint32_t *)spi2_probe_mmio;
    unsigned spi_base_y;
    unsigned spi_spacing;

    clear_screen();
#ifdef SPI2_SETUP
    draw_text(20, 12, "SPI2 SETUP", 0xffffffffu);
#elif defined(SPI2_ENABLE_GATE)
    draw_text(20, 12, "SPI2 GATE", 0xffffffffu);
#else
    draw_text(20, 12, "SPI2 READ", 0xffffffffu);
#endif
    draw_text(190, 12, "PTR", 0xffffffffu);
    draw_hex64(250, 12, (uint64_t)spi2_probe_mmio);
    spi2_probe_stage = 1;
#ifdef SPI2_ENABLE_GATE
    if (spi2_probe_gate == 0) {
        draw_text(20, 42, "NO GATE", 0xffff0000u);
        return;
    }
    volatile uint32_t *gate = (volatile uint32_t *)spi2_probe_gate;
    uint32_t before = gate[0x1d8 / 4];
    draw_text(20, 42, "BEFORE", 0xffffffffu);
    draw_hex(200, 42, before);
    spi2_probe_stage = 8;

    /* Sandcastle PMGR gate enable: request 0xf, wait for status 0xf,
       then set bit 28. Never access SPI2 if the wait times out. */
    gate[0x1d8 / 4] = before | 0x0fu;
    asm volatile("dsb sy" ::: "memory");
    uint32_t status = 0;
    unsigned tries;
    for (tries = 0; tries < 10000; tries++) {
        status = gate[0x1d8 / 4];
        if (((status >> 4) & 0xfu) == 0xfu)
            break;
    }
    if (tries == 10000) {
        draw_text(20, 68, "GATE FAIL", 0xffff0000u);
        draw_hex(200, 68, status);
        return;
    }
    gate[0x1d8 / 4] = status | 0x10000000u;
    asm volatile("dsb sy" ::: "memory");
    draw_text(20, 68, "AFTER", 0xffffffffu);
    draw_hex(200, 68, gate[0x1d8 / 4]);
    spi_base_y = 94;
    spi_spacing = 26;
#ifdef SPI2_SETUP
    touch_spi2_io io = {
        (void *)spi2_probe_mmio, spi2_read_register, spi2_write_register
    };
    spi2_probe_stage = 9;
    if (touch_spi2_prepare(&io, 24000000, 3000000) != TOUCH_SPI2_OK) {
        draw_text(20, 94, "SETUP FAIL", 0xffff0000u);
        return;
    }
#endif
#else
    if (spi2_probe_gate != 0) {
        volatile const uint32_t *gate = (volatile const uint32_t *)spi2_probe_gate;
        draw_text(20, 48, "GATE", 0xffffffffu);
        asm volatile("dsb sy" ::: "memory");
        spi2_probe_stage = 8;
        draw_hex(200, 48, gate[0x1d8 / 4]);
    }
    spi_base_y = spi2_probe_gate ? 78 : 48;
    spi_spacing = spi2_probe_gate ? 30 : 36;
#endif
    for (unsigned i = 0; i < 5; i++) {
        unsigned y = spi_base_y + i * spi_spacing;
        draw_text(20, y, labels[i], 0xffffffffu);
        asm volatile("dsb sy" ::: "memory");
        spi2_probe_stage = 2 + i;
        spi2_probe_values[i] = spi[offsets[i] / 4];
        draw_hex(200, y, spi2_probe_values[i]);
    }
    if (spi2_probe_gate == 0)
        draw_text(20, 216, "DONE", 0xff00ff00u);
    else {
        volatile uint32_t *fb = (volatile uint32_t *)spi2_probe_framebuffer;
        for (unsigned y = 230; y < 237; y++)
            for (unsigned x = 20; x < 160; x++)
                fb[y * STRIDE + x] = 0xff00ff00u;
    }
    spi2_probe_stage = 7;
    asm volatile("dsb sy" ::: "memory");
}
#endif

void notified(microkit_channel channel) { (void)channel; }
