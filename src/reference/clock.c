#include "protocol.h"

#include <stdint.h>

/*
 * QEMU virt's PL031 RTC gives this component an independent wall-clock alarm.
 * The architectural timers are left to the seL4 scheduler.
 */
volatile uintptr_t rtc_regs;

#define RTC_DR    0x00
#define RTC_MR    0x04
#define RTC_CR    0x0c
#define RTC_IMSC  0x10
#define RTC_ICR   0x1c

#define RTC_ALARM_BIT ((uint32_t)1)

static volatile uint32_t *const rtc = (volatile uint32_t *)0x50000000;
static uint32_t starting_seconds;

static void arm_next_second(void)
{
    rtc[RTC_MR / sizeof(*rtc)] = rtc[RTC_DR / sizeof(*rtc)] + 1;
}

void init(void)
{
    /* Enable the free-running counter before setting its next-second alarm. */
    rtc[RTC_CR / sizeof(*rtc)] = 1;
    rtc[RTC_ICR / sizeof(*rtc)] = RTC_ALARM_BIT;
    rtc[RTC_IMSC / sizeof(*rtc)] = RTC_ALARM_BIT;
    starting_seconds = rtc[RTC_DR / sizeof(*rtc)];
    arm_next_second();
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    if (channel != CLOCK_TO_CONSOLE_CHANNEL ||
        microkit_msginfo_get_label(request) != REQUEST_CLOCK_STATUS ||
        microkit_msginfo_get_count(request) != 0) {
        return microkit_msginfo_new(REPLY_CONSOLE_ERROR, 0);
    }

    uint32_t current = rtc[RTC_DR / sizeof(*rtc)];
    microkit_mr_set(0, (seL4_Word)(uint32_t)(current - starting_seconds));
    return microkit_msginfo_new(REPLY_OK, 1);
}

void notified(microkit_channel channel)
{
    if (channel != TIMER_IRQ_CHANNEL) {
        microkit_dbg_puts("CLOCK|ERROR: unexpected notification\n");
        return;
    }

    if ((rtc[0x18 / sizeof(*rtc)] & RTC_ALARM_BIT) != 0) {
        /* Clear and re-arm the level interrupt before notifying the console. */
        rtc[RTC_ICR / sizeof(*rtc)] = RTC_ALARM_BIT;
        arm_next_second();
        microkit_notify(TIMER_TO_CONSOLE_CHANNEL);
    }
    microkit_irq_ack(channel);
}
