/* Read-only Qualcomm GENI SE7 register probe for Pixel 4a touch I2C. */
#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u

uintptr_t touch_i2c_vaddr;
uintptr_t gpio6_vaddr;
uintptr_t gpio7_vaddr;
uintptr_t gpio8_vaddr;
uintptr_t gpio9_vaddr;
uintptr_t gpio72_vaddr;

static void hex32(uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 28; shift >= 0; shift -= 4) {
        microkit_dbg_putc(digits[(value >> shift) & 15u]);
    }
}

static void read_register(const char *name, uint32_t offset)
{
    volatile const uint32_t *reg = (volatile const uint32_t *)(touch_i2c_vaddr + offset);
    microkit_dbg_puts("SUNFISH SE7 ");
    microkit_dbg_puts(name);
    microkit_dbg_putc(' ');
    hex32(*reg);
    microkit_dbg_puts("\r\n");
}

static void read_gpio(const char *name, uintptr_t gpio_vaddr, uint32_t offset)
{
    volatile const uint32_t *reg = (volatile const uint32_t *)(gpio_vaddr + offset);
    microkit_dbg_puts("SUNFISH TLMM ");
    microkit_dbg_puts(name);
    microkit_dbg_putc(' ');
    hex32(*reg);
    microkit_dbg_puts("\r\n");
}


void init(void)
{
    microkit_dbg_puts("\r\nSUNFISH SE7 READ START\r\n");
    read_register("GENI_STATUS", 0x40);
    read_register("OUTPUT_CTRL", 0x24);
    read_register("M_CLK_CFG", 0x48);
    read_register("FW_REVISION", 0x68);
    read_register("CLK_SEL", 0x7c);
    read_register("DMA_MODE", 0x258);
    read_register("SCL_COUNTERS", 0x278);
    read_register("M_IRQ_STATUS", 0x610);
    read_register("M_IRQ_EN", 0x614);
    read_register("TX_FIFO_STATUS", 0x800);
    read_register("RX_FIFO_STATUS", 0x804);
    read_register("I2C_IOS", 0x908);
    read_register("HW_PARAM_0", 0xe24);
    read_register("HW_PARAM_1", 0xe28);
    /* GPIO6/7 belong to the NORTH bank: TLMM base 0x03400000 plus
     * 0x00500000 bank offset plus 0x1000 per GPIO. */
    read_gpio("GPIO6 CFG", gpio6_vaddr, 0x0);
    read_gpio("GPIO6 IO", gpio6_vaddr, 0x4);
    read_gpio("GPIO7 CFG", gpio7_vaddr, 0x0);
    read_gpio("GPIO7 IO", gpio7_vaddr, 0x4);
    read_gpio("GPIO8 CFG", gpio8_vaddr, 0x0);
    read_gpio("GPIO8 IO", gpio8_vaddr, 0x4);
    read_gpio("GPIO9 CFG", gpio9_vaddr, 0x0);
    read_gpio("GPIO9 IO", gpio9_vaddr, 0x4);
    read_gpio("GPIO72 CFG", gpio72_vaddr, 0x0);
    read_gpio("GPIO72 IO", gpio72_vaddr, 0x4);
    microkit_dbg_puts("SUNFISH SE7 RESET REQUEST\r\n");
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_SYSTEM_RESET;
    microkit_arm_smc_call(&args, &response);
    microkit_dbg_puts("SUNFISH SE7 RESET RETURNED\r\n");
}

void notified(microkit_channel channel) { (void)channel; }
