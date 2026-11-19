/*
 * Deterministic wall-clock provider for the isolated AgentStore QEMU test.
 * Production images must use a clock service; this fixture avoids coupling
 * the migration test to the host clock or to a real device RTC.
 */
#include <sys/time.h>
#include <stdint.h>

int gettimeofday(struct timeval *value, void *timezone)
{
    (void)timezone;
    if (value == 0) return -1;
    value->tv_sec = 1798761600; /* 2027-01-01 00:00:00 UTC */
    value->tv_usec = 0;
    return 0;
}

uint16_t htons(uint16_t value) { return __builtin_bswap16(value); }
uint16_t ntohs(uint16_t value) { return __builtin_bswap16(value); }
uint32_t htonl(uint32_t value) { return __builtin_bswap32(value); }
uint32_t ntohl(uint32_t value) { return __builtin_bswap32(value); }
