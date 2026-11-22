/* Platform calls used by the upstream SMP encoding dependencies in the
 * isolated QEMU probe. Time comes from the test clock service, not host libc.
 */
#include <stdbool.h>
#include <stdint.h>
#include <sys/time.h>

extern bool sel4_rts_read_unix_time(uint64_t *seconds, uint32_t *nanoseconds);

int gettimeofday(struct timeval *value, void *timezone)
{
    uint64_t seconds;
    uint32_t nanoseconds;
    (void)timezone;
    if (value == 0 || !sel4_rts_read_unix_time(&seconds, &nanoseconds) ||
        seconds > INT64_MAX || nanoseconds >= 1000000000u)
        return -1;
    value->tv_sec = (time_t)seconds;
    value->tv_usec = (suseconds_t)(nanoseconds / 1000u);
    return 0;
}

uint16_t htons(uint16_t value) { return __builtin_bswap16(value); }
uint16_t ntohs(uint16_t value) { return __builtin_bswap16(value); }
uint32_t htonl(uint32_t value) { return __builtin_bswap32(value); }
uint32_t ntohl(uint32_t value) { return __builtin_bswap32(value); }
