/* Call the service's usual endpoint slot without an installed capability. */
#include <microkit.h>

void init(void)
{
    microkit_dbg_puts("SUNFISH IPC INTRUDER CALL WITHOUT CAP\r\n");
    microkit_msginfo reply = seL4_Call(BASE_ENDPOINT_CAP, microkit_msginfo_new(42, 0));
    microkit_dbg_puts("SUNFISH IPC INTRUDER REPLY LABEL=");
    microkit_dbg_put32(microkit_msginfo_get_label(reply));
    microkit_dbg_puts("\r\n");
    if (microkit_msginfo_get_label(reply) == seL4_InvalidCapability) {
        microkit_dbg_puts("SUNFISH IPC DENIED INVALID CAP\r\n");
    } else {
        microkit_dbg_puts("SUNFISH IPC UNEXPECTED REPLY\r\n");
    }
}

void notified(microkit_channel channel) { (void)channel; }
