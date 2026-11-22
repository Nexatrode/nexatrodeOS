#ifndef GHC_MICROKIT_TIME_H
#define GHC_MICROKIT_TIME_H

/* Compile-time time types for freestanding GHC package probes. */
typedef long time_t;
typedef long clock_t;

struct timespec {
    time_t tv_sec;
    long tv_nsec;
};

struct tm {
    int tm_sec;
    int tm_min;
    int tm_hour;
    int tm_mday;
    int tm_mon;
    int tm_year;
    int tm_wday;
    int tm_yday;
    int tm_isdst;
    long tm_gmtoff;
    const char *tm_zone;
};

struct tm *gmtime_r(const time_t *timer, struct tm *result);
struct tm *localtime_r(const time_t *timer, struct tm *result);
struct tm *localtime(const time_t *timer);
void tzset(void);

#define CLOCKS_PER_SEC 1000000L

#endif
