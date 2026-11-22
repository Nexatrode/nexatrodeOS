/* QEMU-only PL031 calendar-time provider for TLS integration tests.
 * This is host-controlled emulated time, not a production trust source.
 */
#include <stdint.h>
#include <microkit.h>

#define CLOCK_REQUEST 0x501u
#define CLOCK_REPLY 0x1501u

uintptr_t rtc_regs;
static uint32_t last_seconds;

void init(void)
{
    microkit_dbg_puts("TLS_CLOCK|QEMU_RTC_READY\n");
}

void notified(microkit_channel channel)
{
    (void)channel;
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    if (channel != 0 || microkit_msginfo_get_label(request) != CLOCK_REQUEST ||
        microkit_msginfo_get_count(request) != 0 || rtc_regs == 0)
        return microkit_msginfo_new(0, 0);

    uint32_t seconds = *(volatile uint32_t *)rtc_regs;
    /* Reject an uninitialized clock and a backward step within this boot. */
    if (seconds < 1577836800u || (last_seconds != 0 && seconds < last_seconds))
        return microkit_msginfo_new(0, 0);
    last_seconds = seconds;
    microkit_mr_set(0, seconds);
    return microkit_msginfo_new(CLOCK_REPLY, 1);
}
