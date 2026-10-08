/* Pixel 4a FTS touch controller service for the Microkit Slint shell. */
#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u
#define SE_GENI_M_CLK_CFG 0x48u
#define SE_GENI_CLK_CTRL_RO 0x60u
#define SE_GENI_FIFO_DISABLE_RO 0x64u
#define SE_GENI_CLK_SEL 0x7cu
#define SE_GENI_OUTPUT_CTRL 0x24u
#define SE_GENI_DMA_MODE 0x258u
#define SE_I2C_SCL_COUNTERS 0x278u
#define SE_I2C_TX_TRANS_LEN 0x26cu
#define SE_I2C_RX_TRANS_LEN 0x270u
#define SE_GENI_TX_PACKING_CFG0 0x260u
#define SE_GENI_TX_PACKING_CFG1 0x264u
#define SE_GENI_RX_PACKING_CFG0 0x284u
#define SE_GENI_RX_PACKING_CFG1 0x288u
#define SE_GENI_TX_FIFO 0x700u
#define SE_GENI_RX_FIFO 0x780u
#define SE_GENI_RX_FIFO_STATUS 0x804u
#define SE_GENI_TX_WATERMARK 0x80cu
#define SE_GENI_M_CMD0 0x600u
#define SE_GENI_M_IRQ_STATUS 0x610u
#define SE_GENI_M_IRQ_EN 0x614u
#define SE_GENI_M_IRQ_CLEAR 0x618u
#define SE_GENI_IOS 0x908u
#define SE_GENI_STATUS 0x40u

#define I2C_ADDR_ONLY 4u
#define I2C_WRITE 1u
#define I2C_READ 2u
#define TOUCH_ADDRESS 0x49u
#define STOP_STRETCH (1u << 2)
#define CMD_DONE (1u << 0)
#define CMD_FAILURE (1u << 3)
#define RX_FIFO_WATERMARK (1u << 26)
#define RX_FIFO_LAST (1u << 27)
#define TX_FIFO_WATERMARK (1u << 30)
#define I2C_NACK (1u << 10)
#define I2C_BUS_PROTO (1u << 12)
#define I2C_ARB_LOST (1u << 13)
#define GCC_SE7_VOTE (1u << 23)
#define GCC_BRANCH_HALTED (1u << 31)
#define GCC_ROOT_OFF (1u << 31)
#define GCC_ROOT_ENABLE (1u << 1)
#define TOUCH_EVENT_CAPACITY 256u

uintptr_t touch_i2c_vaddr;
uintptr_t gcc_vote_vaddr;
uintptr_t gcc_s1_vaddr;
uintptr_t gpio6_vaddr;
uintptr_t gpio7_vaddr;
uintptr_t gpio8_vaddr;
uintptr_t gpio72_vaddr;
uintptr_t spmi_core_vaddr;
uintptr_t spmi_config_vaddr;
uintptr_t spmi_gpio4_observer_vaddr;
uintptr_t spmi_gpio4_writer_vaddr;
uintptr_t touch_shared_vaddr;

typedef struct {
    volatile uint32_t revision;
    volatile uint32_t x;
    volatile uint32_t y;
} TouchEvent;

typedef struct {
    volatile uint32_t event_write;
    TouchEvent events[TOUCH_EVENT_CAPACITY];
} TouchState;

_Static_assert(sizeof(TouchState) <= 0x1000,
               "touch queue must fit one shared page");

static uint32_t read_reg(uint32_t offset)
{
    return *(volatile uint32_t *)(touch_i2c_vaddr + offset);
}

static void write_reg(uint32_t offset, uint32_t value)
{
    *(volatile uint32_t *)(touch_i2c_vaddr + offset) = value;
}

static void hex32(uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 28; shift >= 0; shift -= 4) {
        microkit_dbg_putc(digits[(value >> shift) & 15u]);
    }
}

static void report(const char *name, uint32_t value)
{
    microkit_dbg_puts("SUNFISH TOUCH PING ");
    microkit_dbg_puts(name);
    microkit_dbg_putc(' ');
    hex32(value);
    microkit_dbg_puts("\r\n");
}

static void publish_contact(uint32_t x, uint32_t y)
{
    volatile TouchState *state = (volatile TouchState *)touch_shared_vaddr;
    uint32_t next = state->event_write + 1u;
    volatile TouchEvent *event = &state->events[(next - 1u) % TOUCH_EVENT_CAPACITY];
    event->revision = (next << 1) | 1u;
    __asm__ volatile("dmb ish" ::: "memory");
    event->x = x;
    event->y = y;
    __asm__ volatile("dmb ish" ::: "memory");
    event->revision = next << 1;
    __asm__ volatile("dmb ish" ::: "memory");
    state->event_write = next;
    microkit_notify(0);
}

static uint32_t spmi_read_gpio4(uint8_t reg)
{
    volatile uint32_t *channel =
        (volatile uint32_t *)(spmi_gpio4_observer_vaddr + 0x200u);
    channel[0] = (1u << 27) | ((uint32_t)reg << 4); /* EXT_READL */
    __asm__ volatile("dmb sy" ::: "memory");
    for (uint32_t polls = 0; polls < 200000u; ++polls) {
        uint32_t status = channel[2];
        if (status & 1u) {
            return status == 1u ? channel[6] & 0xffu : 0xffffffffu;
        }
    }
    return 0xffffffffu;
}

static int spmi_write_gpio4(uint8_t reg, uint8_t value)
{
    volatile uint32_t *channel = (volatile uint32_t *)spmi_gpio4_writer_vaddr;
    channel[4] = value;
    __asm__ volatile("dmb sy" ::: "memory");
    channel[0] = (uint32_t)reg << 4; /* EXT_WRITEL */
    __asm__ volatile("dmb sy" ::: "memory");
    for (uint32_t polls = 0; polls < 200000u; ++polls) {
        uint32_t status = channel[2];
        if (status & 1u) {
            return status == 1u;
        }
    }
    return 0;
}

static int setup_touch_power(void)
{
    volatile const uint32_t *core = (volatile const uint32_t *)spmi_core_vaddr;
    volatile const uint32_t *config = (volatile const uint32_t *)spmi_config_vaddr;
    report("SPMI VERSION", core[0]);
    report("SPMI GPIO4 MAP", core[(0x900u / 4u) + 0xe4u]);
    report("SPMI GPIO4 OWNER", config[(0x700u / 4u) + 0xe4u]);
    if (core[0] != 0x50000000u ||
        core[(0x900u / 4u) + 0xe4u] != 0x0100c300u ||
        (config[(0x700u / 4u) + 0xe4u] & 7u) != 0u) {
        return 0;
    }
    uint32_t type = spmi_read_gpio4(0x04u);
    uint32_t subtype = spmi_read_gpio4(0x05u);
    uint32_t mode = spmi_read_gpio4(0x40u);
    uint32_t vin = spmi_read_gpio4(0x41u);
    uint32_t pull = spmi_read_gpio4(0x42u);
    uint32_t source = spmi_read_gpio4(0x44u);
    uint32_t out = spmi_read_gpio4(0x45u);
    uint32_t enable = spmi_read_gpio4(0x46u);
    report("POWER TYPE", type);
    report("POWER SUBTYPE", subtype);
    report("POWER MODE BEFORE", mode);
    if (type != 0x10u || subtype != 0x11u || enable != 0x80u) {
        return 0;
    }
    if (mode == 0u && (vin == 0u || vin == 1u) && pull == 4u &&
        source == 0u && out == 1u) {
        if (!spmi_write_gpio4(0x41u, 1u) ||
            !spmi_write_gpio4(0x42u, 5u) ||
            !spmi_write_gpio4(0x45u, 2u) ||
            !spmi_write_gpio4(0x44u, 0x80u) ||
            !spmi_write_gpio4(0x40u, 1u)) {
            return 0;
        }
    } else if (mode != 1u) {
        return 0;
    }
    int ready = spmi_read_gpio4(0x41u) == 1u &&
                spmi_read_gpio4(0x42u) == 5u &&
                spmi_read_gpio4(0x45u) == 2u &&
                spmi_read_gpio4(0x44u) == 0x80u &&
                spmi_read_gpio4(0x40u) == 1u &&
                spmi_read_gpio4(0x46u) == 0x80u;
    report("POWER READY", (uint32_t)ready);
    return ready;
}

void init(void)
{
    microkit_dbg_puts("\r\nSUNFISH TOUCH PING START\r\n");
    report("IOS BEFORE", read_reg(SE_GENI_IOS));
    report("STATUS BEFORE", read_reg(SE_GENI_STATUS));
    report("IRQ BEFORE", read_reg(SE_GENI_M_IRQ_STATUS));
    volatile uint32_t *gcc_vote = (volatile uint32_t *)(gcc_vote_vaddr + 0x0cu);
    volatile uint32_t *gcc_halt = (volatile uint32_t *)(gcc_s1_vaddr + 0x144u);
    volatile uint32_t *gcc_source_cmd = (volatile uint32_t *)(gcc_s1_vaddr + 0x148u);
    report("GCC VOTE BEFORE", *gcc_vote);
    report("GCC S1 HALT", *gcc_halt);
    report("GCC S1 CMD", *gcc_source_cmd);
    report("GCC S1 CFG", *(volatile uint32_t *)(gcc_s1_vaddr + 0x14cu));

    /* Linux's gcc-sdmmagpie clock branch uses bit 23 of this shared vote
     * register for gcc_qupv3_wrap1_s1_clk. Preserve all other votes. */
    *gcc_vote = *gcc_vote | GCC_SE7_VOTE;
    report("GCC VOTE AFTER", *gcc_vote);
    uint32_t clock_polls = 0;
    for (; clock_polls < 2000000u; ++clock_polls) {
        if ((*gcc_halt & GCC_BRANCH_HALTED) == 0) {
            break;
        }
    }
    report("GCC HALT AFTER", *gcc_halt);
    report("GCC POLLS", clock_polls);
    report("GCC S1 CMD AFTER", *gcc_source_cmd);
    if (*gcc_source_cmd & GCC_ROOT_OFF) {
        /* The source RCG may also be off. Linux's clk-rcg2 driver uses
         * CMD_ROOT_EN to force it on while switching clock sources. */
        *gcc_source_cmd = *gcc_source_cmd | GCC_ROOT_ENABLE;
        uint32_t root_polls = 0;
        for (; root_polls < 2000000u; ++root_polls) {
            if ((*gcc_source_cmd & GCC_ROOT_OFF) == 0) {
                break;
            }
        }
        report("GCC ROOT POLLS", root_polls);
        report("GCC S1 CMD FORCED", *gcc_source_cmd);
    }
    report("IOS CLOCKED", read_reg(SE_GENI_IOS));
    report("M CLK BEFORE", read_reg(SE_GENI_M_CLK_CFG));
    report("CLK CTRL", read_reg(SE_GENI_CLK_CTRL_RO));
    report("FIFO DISABLE", read_reg(SE_GENI_FIFO_DISABLE_RO));
    report("OUTPUT CTRL", read_reg(SE_GENI_OUTPUT_CTRL));
    report("DMA MODE", read_reg(SE_GENI_DMA_MODE));
    volatile uint32_t *gpio6_cfg = (volatile uint32_t *)gpio6_vaddr;
    volatile uint32_t *gpio7_cfg = (volatile uint32_t *)gpio7_vaddr;
    report("GPIO6 CFG BEFORE", *gpio6_cfg);
    report("GPIO7 CFG BEFORE", *gpio7_cfg);
    if ((*gcc_halt & GCC_BRANCH_HALTED) || (*gcc_source_cmd & GCC_ROOT_OFF)) {
        goto reset;
    }
    if (!setup_touch_power()) {
        goto reset;
    }

    /* In pinctrl-sdmmagpie, qup11 is mux selector 1 and the mux field is
     * bits 5:2. Bias-disable and 2 mA drive are both zero in the DT. */
    *gpio6_cfg = (*gpio6_cfg & ~0xffu) | 0x4u;
    *gpio7_cfg = (*gpio7_cfg & ~0xffu) | 0x4u;
    report("GPIO6 CFG AFTER", *gpio6_cfg);
    report("GPIO7 CFG AFTER", *gpio7_cfg);
    volatile uint32_t *gpio8_cfg = (volatile uint32_t *)gpio8_vaddr;
    volatile uint32_t *gpio8_io = (volatile uint32_t *)(gpio8_vaddr + 0x4u);
    volatile uint32_t *gpio72_cfg = (volatile uint32_t *)gpio72_vaddr;
    volatile uint32_t *gpio72_io = (volatile uint32_t *)(gpio72_vaddr + 0x4u);
    report("GPIO8 CFG BEFORE", *gpio8_cfg);
    report("GPIO72 CFG BEFORE", *gpio72_cfg);
    /* fts_set_gpio drives reset high; FTS_SWITCH_GPIO_VALUE_AP_MASTER is 0.
     * Set output data before enabling each output to avoid a transient. */
    *gpio72_io = 0u;
    *gpio72_cfg = (*gpio72_cfg & ~0x3ffu) | (1u << 9);
    *gpio8_io = (1u << 1);
    *gpio8_cfg = (*gpio8_cfg & ~0x3ffu) | (1u << 9);
    for (volatile uint32_t settle = 0; settle < 1000000u; ++settle) { }
    report("GPIO8 CFG AFTER", *gpio8_cfg);
    report("GPIO8 IO AFTER", *gpio8_io);
    report("GPIO72 CFG AFTER", *gpio72_cfg);
    report("GPIO72 IO AFTER", *gpio72_io);
    report("IOS ROUTED", read_reg(SE_GENI_IOS));
    if ((read_reg(SE_GENI_IOS) & 0x3u) != 0x3u) {
        goto reset;
    }

    /* Match the Linux driver's 1 MHz sequencer-clock and SCL settings.
     * The prior probe set only SCL counters; its command stayed active. */
    write_reg(SE_GENI_CLK_SEL, 0u);
    write_reg(SE_GENI_M_CLK_CFG, (1u << 4) | 1u);
    write_reg(SE_I2C_SCL_COUNTERS, (1u << 20) | (5u << 10) | 13u);
    write_reg(SE_GENI_M_IRQ_CLEAR, 0xffffffffu);
    write_reg(SE_GENI_M_IRQ_EN, read_reg(SE_GENI_M_IRQ_EN) |
              CMD_DONE | CMD_FAILURE | I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST |
              RX_FIFO_WATERMARK | RX_FIFO_LAST | TX_FIFO_WATERMARK);
    report("SCL COUNTERS", read_reg(SE_I2C_SCL_COUNTERS));
    report("M CLK AFTER", read_reg(SE_GENI_M_CLK_CFG));

    /* No data phase: send only the 7-bit address, then stop. */
    write_reg(SE_GENI_M_CMD0, (I2C_ADDR_ONLY << 27) | (TOUCH_ADDRESS << 9));
    uint32_t irq = 0;
    uint32_t attempts = 0;
    for (; attempts < 2000000u; ++attempts) {
        irq = read_reg(SE_GENI_M_IRQ_STATUS);
        if (irq & (CMD_DONE | CMD_FAILURE | I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST)) {
            break;
        }
    }
    report("IRQ AFTER", irq);
    report("STATUS AFTER", read_reg(SE_GENI_STATUS));
    report("IOS AFTER", read_reg(SE_GENI_IOS));
    report("POLLS", attempts);
    if ((irq & CMD_DONE) == 0 ||
        (irq & (CMD_FAILURE | I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST))) {
        goto reset;
    }

    /* Linux's 8-bit, 4-byte-per-FIFO-word packing configuration. */
    write_reg(SE_GENI_TX_PACKING_CFG0, 0x0007f8feu);
    write_reg(SE_GENI_TX_PACKING_CFG1, 0x000ffefeu);
    write_reg(SE_GENI_RX_PACKING_CFG0, 0x0007f8feu);
    write_reg(SE_GENI_RX_PACKING_CFG1, 0x000ffefeu);
    for (volatile uint32_t settle = 0; settle < 20000000u; ++settle) { }

    /* FTS FIFO event read: write register 0x86 while holding the bus for
     * a repeated start, then read one eight-byte event packet. */
    write_reg(SE_GENI_M_IRQ_CLEAR, 0xffffffffu);
    write_reg(SE_I2C_TX_TRANS_LEN, 1u);
    write_reg(SE_GENI_TX_WATERMARK, 1u);
    write_reg(SE_GENI_M_CMD0,
              (I2C_WRITE << 27) | (TOUCH_ADDRESS << 9) | STOP_STRETCH);
    irq = 0;
    for (attempts = 0; attempts < 2000000u; ++attempts) {
        irq = read_reg(SE_GENI_M_IRQ_STATUS);
        if (irq & (TX_FIFO_WATERMARK | CMD_DONE | CMD_FAILURE |
                   I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST)) {
            break;
        }
    }
    report("WRITE READY IRQ", irq);
    if ((irq & TX_FIFO_WATERMARK) == 0) {
        goto reset;
    }
    write_reg(SE_GENI_TX_FIFO, 0x86u);
    write_reg(SE_GENI_TX_WATERMARK, 0u);
    write_reg(SE_GENI_M_IRQ_CLEAR, TX_FIFO_WATERMARK);
    for (attempts = 0; attempts < 2000000u; ++attempts) {
        irq = read_reg(SE_GENI_M_IRQ_STATUS);
        if (irq & (CMD_DONE | CMD_FAILURE | I2C_NACK |
                   I2C_BUS_PROTO | I2C_ARB_LOST)) {
            break;
        }
    }
    report("WRITE IRQ", irq);
    report("WRITE POLLS", attempts);
    if ((irq & CMD_DONE) == 0 ||
        (irq & (CMD_FAILURE | I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST))) {
        goto reset;
    }

    write_reg(SE_GENI_M_IRQ_CLEAR, 0xffffffffu);
    write_reg(SE_I2C_RX_TRANS_LEN, 8u);
    write_reg(SE_GENI_M_CMD0, (I2C_READ << 27) | (TOUCH_ADDRESS << 9));
    irq = 0;
    for (attempts = 0; attempts < 2000000u; ++attempts) {
        irq = read_reg(SE_GENI_M_IRQ_STATUS);
        if (irq & (CMD_DONE | CMD_FAILURE | I2C_NACK |
                   I2C_BUS_PROTO | I2C_ARB_LOST)) {
            break;
        }
    }
    report("READ IRQ", irq);
    report("READ POLLS", attempts);
    uint32_t fifo_status = read_reg(SE_GENI_RX_FIFO_STATUS);
    report("READ FIFO STATUS", fifo_status);
    if ((irq & CMD_DONE) &&
        !(irq & (CMD_FAILURE | I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST)) &&
        (fifo_status & 0x01ffffffu) >= 2u) {
        report("EVENT WORD0", read_reg(SE_GENI_RX_FIFO));
        report("EVENT WORD1", read_reg(SE_GENI_RX_FIFO));
    } else {
        goto reset;
    }

    /* Linux's fts_mode_handler enables active sensing with A0 00 FF.
     * Reading the FIFO alone only returned the controller-ready event. */
    write_reg(SE_GENI_M_IRQ_CLEAR, 0xffffffffu);
    write_reg(SE_I2C_TX_TRANS_LEN, 3u);
    write_reg(SE_GENI_TX_WATERMARK, 1u);
    write_reg(SE_GENI_M_CMD0, (I2C_WRITE << 27) | (TOUCH_ADDRESS << 9));
    for (attempts = 0; attempts < 2000000u; ++attempts) {
        irq = read_reg(SE_GENI_M_IRQ_STATUS);
        if (irq & (TX_FIFO_WATERMARK | CMD_DONE | CMD_FAILURE |
                   I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST)) {
            break;
        }
    }
    report("SCAN READY IRQ", irq);
    if ((irq & TX_FIFO_WATERMARK) == 0) {
        goto reset;
    }
    write_reg(SE_GENI_TX_FIFO, 0x00ff00a0u);
    write_reg(SE_GENI_TX_WATERMARK, 0u);
    write_reg(SE_GENI_M_IRQ_CLEAR, TX_FIFO_WATERMARK);
    for (attempts = 0; attempts < 2000000u; ++attempts) {
        irq = read_reg(SE_GENI_M_IRQ_STATUS);
        if (irq & (CMD_DONE | CMD_FAILURE | I2C_NACK |
                   I2C_BUS_PROTO | I2C_ARB_LOST)) {
            break;
        }
    }
    report("SCAN IRQ", irq);
    if ((irq & CMD_DONE) == 0 ||
        (irq & (CMD_FAILURE | I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST))) {
        goto reset;
    }

    ((volatile TouchState *)touch_shared_vaddr)->event_write = 0;
    microkit_dbg_puts("SUNFISH TOUCH PING TOUCH READY\r\n");
    uint32_t interesting_events = 0;
    for (uint32_t poll = 0; poll < 100000u; ++poll) {
        for (volatile uint32_t delay = 0; delay < 200000u; ++delay) { }
        write_reg(SE_GENI_M_IRQ_CLEAR, 0xffffffffu);
        write_reg(SE_I2C_TX_TRANS_LEN, 1u);
        write_reg(SE_GENI_TX_WATERMARK, 1u);
        write_reg(SE_GENI_M_CMD0,
                  (I2C_WRITE << 27) | (TOUCH_ADDRESS << 9) | STOP_STRETCH);
        irq = 0;
        for (attempts = 0; attempts < 2000000u; ++attempts) {
            irq = read_reg(SE_GENI_M_IRQ_STATUS);
            if (irq & (TX_FIFO_WATERMARK | CMD_DONE | CMD_FAILURE |
                       I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST)) {
                break;
            }
        }
        if ((irq & TX_FIFO_WATERMARK) == 0) {
            report("POLL WRITE READY ERROR", irq);
            goto reset;
        }
        write_reg(SE_GENI_TX_FIFO, 0x86u);
        write_reg(SE_GENI_TX_WATERMARK, 0u);
        write_reg(SE_GENI_M_IRQ_CLEAR, TX_FIFO_WATERMARK);
        for (attempts = 0; attempts < 2000000u; ++attempts) {
            irq = read_reg(SE_GENI_M_IRQ_STATUS);
            if (irq & (CMD_DONE | CMD_FAILURE | I2C_NACK |
                       I2C_BUS_PROTO | I2C_ARB_LOST)) {
                break;
            }
        }
        if ((irq & CMD_DONE) == 0 ||
            (irq & (CMD_FAILURE | I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST))) {
            report("POLL WRITE ERROR", irq);
            goto reset;
        }
        write_reg(SE_GENI_M_IRQ_CLEAR, 0xffffffffu);
        write_reg(SE_I2C_RX_TRANS_LEN, 8u);
        write_reg(SE_GENI_M_CMD0, (I2C_READ << 27) | (TOUCH_ADDRESS << 9));
        for (attempts = 0; attempts < 2000000u; ++attempts) {
            irq = read_reg(SE_GENI_M_IRQ_STATUS);
            if (irq & (CMD_DONE | CMD_FAILURE | I2C_NACK |
                       I2C_BUS_PROTO | I2C_ARB_LOST)) {
                break;
            }
        }
        if ((irq & CMD_DONE) == 0 ||
            (irq & (CMD_FAILURE | I2C_NACK | I2C_BUS_PROTO | I2C_ARB_LOST))) {
            report("POLL READ ERROR", irq);
            goto reset;
        }
        fifo_status = read_reg(SE_GENI_RX_FIFO_STATUS);
        if ((fifo_status & 0x01ffffffu) < 2u) {
            report("POLL FIFO ERROR", fifo_status);
            goto reset;
        }
        uint32_t word0 = read_reg(SE_GENI_RX_FIFO);
        uint32_t word1 = read_reg(SE_GENI_RX_FIFO);
        uint32_t event = word0 & 0xffu;
        uint32_t tool = (word0 >> 8) & 0x0fu;
        if (event != 0x03u && interesting_events++ < 8u) {
            report("RAW WORD0", word0);
            report("RAW WORD1", word1);
            report("RAW POLL", poll);
        }
        if ((event >> 4) == 1u && tool == 1u) {
            uint32_t x = ((word0 >> 24) & 0x0fu) << 8 |
                         ((word0 >> 16) & 0xffu);
            uint32_t y = (word1 & 0xffu) << 4 |
                         ((word0 >> 28) & 0x0fu);
            report("TOUCH DOWN WORD0", word0);
            report("TOUCH DOWN WORD1", word1);
            report("TOUCH DOWN X", x);
            report("TOUCH DOWN Y", y);
            report("TOUCH DOWN POLL", poll);
            publish_contact(x, y);
        }
    }
    microkit_dbg_puts("SUNFISH TOUCH PING TOUCH TIMEOUT\r\n");
reset:
    microkit_dbg_puts("SUNFISH TOUCH PING RESET REQUEST\r\n");
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
}

void notified(microkit_channel channel) { (void)channel; }
