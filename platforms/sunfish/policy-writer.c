#include <microkit.h>
#include "policy-protocol.h"

void init(void)
{
    microkit_mr_set(0, POLICY_TEST_VALUE);
    microkit_msginfo reply = microkit_ppcall(0, microkit_msginfo_new(POLICY_WRITE, 1));
    if (microkit_msginfo_get_label(reply) != POLICY_OK ||
        microkit_msginfo_get_count(reply) != 0) {
        microkit_dbg_puts("SUNFISH POLICY WRITER FAIL WRITE\r\n");
        return;
    }
    reply = microkit_ppcall(0, microkit_msginfo_new(POLICY_READ, 0));
    if (microkit_msginfo_get_label(reply) == POLICY_OK &&
        microkit_msginfo_get_count(reply) == 1 &&
        microkit_mr_get(0) == POLICY_TEST_VALUE) {
        microkit_dbg_puts("SUNFISH POLICY WRITER PASS\r\n");
    } else {
        microkit_dbg_puts("SUNFISH POLICY WRITER FAIL READ\r\n");
    }
}

void notified(microkit_channel channel) { (void)channel; }
