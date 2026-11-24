/* Disposable offline service: no sockets, peers, credentials or remote relay. */
#include <microkit.h>
void init(void) {}
void notified(microkit_channel channel) { (void)channel; }
microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    if ((channel == 0 || channel == 2) && microkit_msginfo_get_label(request) == 0x205 && !microkit_msginfo_get_count(request)) {
        microkit_mr_set(0, 1795392000); /* Fixed disposable timestamp. */
        return microkit_msginfo_new(0x100, 1);
    }
    return microkit_msginfo_new(0x304, 0);
}
