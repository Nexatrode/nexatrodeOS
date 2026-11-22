#include <microkit.h>
#include "policy-protocol.h"

void init(void)
{
    microkit_msginfo reply = seL4_Call(BASE_ENDPOINT_CAP, microkit_msginfo_new(POLICY_READ, 0));
    if (microkit_msginfo_get_label(reply) == seL4_InvalidCapability) {
        microkit_dbg_puts("SUNFISH POLICY INTRUDER DENIED NO CAP\r\n");
    } else {
        microkit_dbg_puts("SUNFISH POLICY INTRUDER UNEXPECTED REPLY\r\n");
    }
}

void notified(microkit_channel channel) { (void)channel; }
