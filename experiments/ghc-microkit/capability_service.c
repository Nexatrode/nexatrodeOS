/* SPDX-License-Identifier: BSD-2-Clause */
#include <microkit.h>

#define REQUEST_LABEL 0x701u
#define REPLY_LABEL 0x1701u
#define REPLY_VALUE 0x53454c3447424348ull

void init(void)
{
    microkit_dbg_puts("GHCRTS|CAPABILITY_SERVICE_READY\n");
}

void notified(microkit_channel channel)
{
    (void)channel;
    microkit_dbg_puts("GHCRTS|FAIL: unexpected service notification\n");
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo msginfo)
{
    if (channel != 0 || microkit_msginfo_get_label(msginfo) != REQUEST_LABEL ||
        microkit_msginfo_get_count(msginfo) != 0) {
        return microkit_msginfo_new(0, 0);
    }

    microkit_mr_set(0, REPLY_VALUE);
    return microkit_msginfo_new(REPLY_LABEL, 1);
}
