#include <stdint.h>
#include <microkit.h>

static volatile uint32_t *const result = (volatile uint32_t *)0x40000000;

void init(void)
{
    result[1] = 0;
    microkit_dbg_puts("SUNFISH IPC SERVICE READY\r\n");
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    if (channel == 0 && microkit_msginfo_get_label(request) == 42 &&
        microkit_msginfo_get_count(request) == 1 && microkit_mr_get(0) == 41) {
        result[1]++;
        microkit_mr_set(0, 42);
        return microkit_msginfo_new(43, 1);
    }
    return microkit_msginfo_new(0, 0);
}

void notified(microkit_channel channel) { (void)channel; }
