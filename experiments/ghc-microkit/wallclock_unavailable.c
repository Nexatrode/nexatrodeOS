/*
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * This PD has a monotonic timer but no wall-clock service. Preserve that
 * distinction: GHC's GetTime hook turns false into an explicit runtime error
 * if an application asks for Unix epoch time.
 */
#include <stdbool.h>
#include <stdint.h>

bool sel4_rts_read_unix_time(uint64_t *seconds, uint32_t *nanoseconds)
{
    (void)seconds;
    (void)nanoseconds;
    return false;
}
