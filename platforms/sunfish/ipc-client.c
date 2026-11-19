#include <stdint.h>
#include <microkit.h>

static volatile uint32_t *const result = (volatile uint32_t *)0x40000000;

void init(void)
{
    result[0] = 0;
    microkit_dbg_puts("SUNFISH IPC CLIENT CALL\r\n");
    microkit_mr_set(0, 41);
    microkit_msginfo reply = microkit_ppcall(0, microkit_msginfo_new(42, 1));
    if (microkit_msginfo_get_label(reply) == 43 &&
        microkit_msginfo_get_count(reply) == 1 && microkit_mr_get(0) == 42) {
        result[0] = 1;
        microkit_dbg_puts("SUNFISH IPC CLIENT PASS\r\n");
    } else {
        result[0] = 2;
        microkit_dbg_puts("SUNFISH IPC CLIENT FAIL\r\n");
    }
}

void notified(microkit_channel channel) { (void)channel; }
