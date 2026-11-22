/* SPDX-License-Identifier: BSD-2-Clause */

#include <microkit.h>

static volatile unsigned int started;

void init(void)
{
    started = 1;
}

void notified(microkit_channel channel)
{
    (void)channel;
}
