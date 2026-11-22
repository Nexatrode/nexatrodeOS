#include <time.h>

static int check(time_t seconds, int year, int month, int day,
                 int hour, int minute, int second, int weekday, int yearday)
{
    struct tm calendar;
    if (gmtime_r(&seconds, &calendar) != &calendar) return 0;
    if (calendar.tm_year != year - 1900 || calendar.tm_mon != month - 1 ||
        calendar.tm_mday != day || calendar.tm_hour != hour ||
        calendar.tm_min != minute || calendar.tm_sec != second ||
        calendar.tm_wday != weekday || calendar.tm_yday != yearday ||
        calendar.tm_isdst != 0 || calendar.tm_gmtoff != 0) return 0;
    if (localtime_r(&seconds, &calendar) != &calendar) return 0;
    return calendar.tm_year == year - 1900 && calendar.tm_mon == month - 1;
}

int main(void)
{
    if (!check(0, 1970, 1, 1, 0, 0, 0, 4, 0)) return 1;
    if (!check(-1, 1969, 12, 31, 23, 59, 59, 3, 364)) return 2;
    if (!check(951782400, 2000, 2, 29, 0, 0, 0, 2, 59)) return 3;
    if (!check(1709164800, 2024, 2, 29, 0, 0, 0, 4, 59)) return 4;
    return 0;
}
