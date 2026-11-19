/* This hook exists only in the QEMU TLS test image. */
#include <stdbool.h>
#include <stdint.h>
#include <microkit.h>

#ifndef CLOCK_CHANNEL
#define CLOCK_CHANNEL 3
#endif
#ifndef CLOCK_REQUEST
#define CLOCK_REQUEST 0x501u
#endif
#ifndef CLOCK_REPLY
#define CLOCK_REPLY 0x1501u
#endif

bool sel4_rts_read_unix_time(uint64_t *seconds, uint32_t *nanoseconds)
{
    if (seconds == 0 || nanoseconds == 0) return false;
    microkit_msginfo reply = microkit_ppcall(
        CLOCK_CHANNEL, microkit_msginfo_new(CLOCK_REQUEST, 0));
    if (microkit_msginfo_get_label(reply) != CLOCK_REPLY ||
        microkit_msginfo_get_count(reply) != 1)
        return false;
    *seconds = microkit_mr_get(0);
    *nanoseconds = 0;
    return true;
}
