#ifndef GUEST_TIME_H
#define GUEST_TIME_H
typedef long time_t;
typedef long clock_t;
typedef long suseconds_t;
struct timespec { time_t tv_sec; long tv_nsec; };
struct tm {
    int tm_sec, tm_min, tm_hour, tm_mday, tm_mon, tm_year, tm_wday, tm_yday, tm_isdst;
};
#ifdef __cplusplus
extern "C" {
#endif
time_t time(time_t *);
double difftime(time_t, time_t);
struct tm *gmtime(const time_t *);
struct tm *localtime(const time_t *);
time_t mktime(struct tm *);
char *asctime(const struct tm *);
char *ctime(const time_t *);
size_t strftime(char *, size_t, const char *, const struct tm *);
clock_t clock(void);
int nanosleep(const struct timespec *, struct timespec *);
#define CLOCK_REALTIME 0
#define CLOCK_MONOTONIC 1
#define CLOCKS_PER_SEC 1000000
int clock_gettime(int, struct timespec *);
#ifdef __cplusplus
}
#endif
#endif
