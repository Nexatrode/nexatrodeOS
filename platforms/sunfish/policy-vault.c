#include <stdint.h>
#include <microkit.h>
#include "policy-protocol.h"

/* value, accepted writes, accepted reads, denied writes */
static volatile uint32_t *const state = (volatile uint32_t *)0x40000000;

void init(void)
{
    for (unsigned i = 0; i < 4; ++i) state[i] = 0;
    microkit_dbg_puts("SUNFISH POLICY VAULT READY\r\n");
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    const seL4_Word label = microkit_msginfo_get_label(request);
    const seL4_Word count = microkit_msginfo_get_count(request);
    if (label == POLICY_READ && count == 0 && (channel == 0 || channel == 1)) {
        ++state[2];
        microkit_mr_set(0, state[0]);
        return microkit_msginfo_new(POLICY_OK, 1);
    }
    if (label == POLICY_WRITE && count == 1) {
        if (channel != 0) {
            ++state[3];
            return microkit_msginfo_new(POLICY_DENIED, 0);
        }
        state[0] = (uint32_t)microkit_mr_get(0);
        ++state[1];
        return microkit_msginfo_new(POLICY_OK, 0);
    }
    return microkit_msginfo_new(POLICY_BAD_REQUEST, 0);
}

void notified(microkit_channel channel) { (void)channel; }
