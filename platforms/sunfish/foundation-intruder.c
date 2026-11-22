/* Expected fault: this PD has no mapping for the other PDs' shared page. */
#include <stdint.h>
#include <microkit.h>

void init(void)
{
    microkit_dbg_puts("SUNFISH FOUNDATION INTRUDER UNMAPPED READ\r\n");
    volatile uint32_t value = *(volatile uint32_t *)0x40000000;
    (void)value;
    microkit_dbg_puts("SUNFISH FOUNDATION ISOLATION FAILED\r\n");
}

void notified(microkit_channel channel) { (void)channel; }
