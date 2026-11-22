/* Bounded PM6150 GPIO4 touch-power probe with exact baseline gating. */
#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u
#define POWER_WRITE_ENABLED 1u

uintptr_t spmi_core_vaddr;
uintptr_t spmi_config_vaddr;
uintptr_t spmi_gpio4_observer_vaddr;
uintptr_t spmi_gpio4_writer_vaddr;

static void hex32(uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 28; shift >= 0; shift -= 4) {
        microkit_dbg_putc(digits[(value >> shift) & 15u]);
    }
}

static void report(const char *name, uint32_t value)
{
    microkit_dbg_puts("SUNFISH SPMI PROBE ");
    microkit_dbg_puts(name);
    microkit_dbg_putc(' ');
    hex32(value);
    microkit_dbg_puts("\r\n");
}

static uint32_t read_pmic_gpio4(const char *name, uint8_t register_offset)
{
    /* v5 observer offset = 0x80 * APID (0xE4) = 0x7200, so the
     * 0xE607000 page's +0x200 points to this channel. */
    volatile uint32_t *channel =
        (volatile uint32_t *)(spmi_gpio4_observer_vaddr + 0x200u);
    /* The PMIC register address is 0xC300 + offset. Linux's SPMI regmap
     * uses EXT_READL (arbiter opcode 1) for addresses above 0x00FF. */
    uint32_t command = (1u << 27) | ((uint32_t)register_offset << 4);
    channel[0] = command;
    __asm__ volatile("dmb sy" ::: "memory");
    uint32_t status = 0;
    uint32_t polls = 0;
    for (; polls < 200000u; ++polls) {
        status = channel[2];
        if (status & 0x1u) {
            break;
        }
    }
    report("READ STATUS", status);
    report("READ POLLS", polls);
    if (status == 0x1u) {
        uint32_t value = channel[6] & 0xffu;
        report(name, value);
        return value;
    }
    return 0xffffffffu;
}

static int write_pmic_gpio4(const char *name, uint8_t register_offset,
                            uint8_t value)
{
    /* v5 RW offset = 0x10000 * APID (0xE4). The mapped page is the
     * PMIC arbiter write channel for exactly this peripheral. */
    volatile uint32_t *channel =
        (volatile uint32_t *)spmi_gpio4_writer_vaddr;
    channel[4] = value;
    __asm__ volatile("dmb sy" ::: "memory");
    channel[0] = ((uint32_t)register_offset << 4); /* EXT_WRITEL opcode 0 */
    __asm__ volatile("dmb sy" ::: "memory");
    uint32_t status = 0;
    uint32_t polls = 0;
    for (; polls < 200000u; ++polls) {
        status = channel[2];
        if (status & 0x1u) {
            break;
        }
    }
    report(name, status);
    return status == 0x1u;
}

void init(void)
{
    microkit_dbg_puts("\r\nSUNFISH SPMI PROBE START\r\n");
    microkit_dbg_puts("SUNFISH SPMI PROBE BEFORE VERSION\r\n");
    volatile const uint32_t *core = (volatile const uint32_t *)spmi_core_vaddr;
    volatile const uint32_t *config = (volatile const uint32_t *)spmi_config_vaddr;
    uint32_t version = core[0];
    report("VERSION", version);
    if ((version & 0xf0000000u) == 0x50000000u) {
        /* PM6150 SID 0, GPIO4 peripheral 0xC3 => PPID 0x0C3. */
        uint32_t gpio4_apid = 0xffffffffu;
        uint32_t gpio4_owner = 0xffffffffu;
        for (uint32_t apid = 0; apid < 512u; ++apid) {
            uint32_t entry = core[(0x900u / 4u) + apid];
            if (entry && ((entry >> 8) & 0xfffu) == 0x0c3u) {
                report("GPIO4 APID", apid);
                report("GPIO4 MAP", entry);
                gpio4_owner = config[(0x700u / 4u) + apid];
                report("GPIO4 OWNER", gpio4_owner);
                gpio4_apid = apid;
            }
        }
        if (gpio4_apid == 0xe4u && (gpio4_owner & 0x7u) == 0u) {
            uint32_t type = read_pmic_gpio4("GPIO4 TYPE", 0x04u);
            uint32_t subtype = read_pmic_gpio4("GPIO4 SUBTYPE", 0x05u);
            uint32_t mode = read_pmic_gpio4("GPIO4 MODE", 0x40u);
            uint32_t vin = read_pmic_gpio4("GPIO4 VIN", 0x41u);
            uint32_t pull = read_pmic_gpio4("GPIO4 PULL", 0x42u);
            uint32_t source = read_pmic_gpio4("GPIO4 SOURCE", 0x44u);
            uint32_t out = read_pmic_gpio4("GPIO4 OUT", 0x45u);
            uint32_t enable = read_pmic_gpio4("GPIO4 ENABLE", 0x46u);
            if (POWER_WRITE_ENABLED &&
                type == 0x10u && subtype == 0x11u && mode == 0u &&
                (vin == 0u || vin == 1u) && pull == 4u && source == 0u &&
                out == 1u && enable == 0x80u) {
                /* Live DT: power-source=1, bias-disable, strength=2,
                 * function=normal, output-high. For MV GPIO, output-high
                 * sets bit 7 of SOURCE; digital output sets MODE=1. */
                if (write_pmic_gpio4("WRITE VIN", 0x41u, 1u) &&
                    write_pmic_gpio4("WRITE PULL", 0x42u, 5u) &&
                    write_pmic_gpio4("WRITE OUT", 0x45u, 2u) &&
                    write_pmic_gpio4("WRITE SOURCE", 0x44u, 0x80u) &&
                    write_pmic_gpio4("WRITE MODE", 0x40u, 1u)) {
                    uint32_t vin_after = read_pmic_gpio4("GPIO4 VIN AFTER", 0x41u);
                    uint32_t pull_after = read_pmic_gpio4("GPIO4 PULL AFTER", 0x42u);
                    uint32_t out_after = read_pmic_gpio4("GPIO4 OUT AFTER", 0x45u);
                    uint32_t source_after = read_pmic_gpio4("GPIO4 SOURCE AFTER", 0x44u);
                    uint32_t mode_after = read_pmic_gpio4("GPIO4 MODE AFTER", 0x40u);
                    uint32_t enable_after = read_pmic_gpio4("GPIO4 ENABLE AFTER", 0x46u);
                    if (vin_after == 1u && pull_after == 5u &&
                        out_after == 2u && source_after == 0x80u &&
                        mode_after == 1u && enable_after == 0x80u) {
                        microkit_dbg_puts("SUNFISH SPMI PROBE POWER READY\r\n");
                    }
                }
            } else {
                microkit_dbg_puts("SUNFISH SPMI PROBE BASELINE MISMATCH\r\n");
            }
        }
    }
    microkit_dbg_puts("SUNFISH SPMI PROBE RESET REQUEST\r\n");
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
}

void notified(microkit_channel channel) { (void)channel; }
