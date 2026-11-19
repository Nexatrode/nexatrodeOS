/* UTC calendar conversion for the freestanding TLS/X.509 dependency stack.
 * The guest has no configured local timezone, so localtime_r is UTC too.
 * Conversion is separate from time acquisition: no clock value is invented.
 */
#include <stdint.h>
#include <time.h>

struct tm *gmtime_r(const time_t *timer, struct tm *result)
{
    if (timer == 0 || result == 0) return 0;
    int64_t days = (int64_t)*timer / 86400;
    int64_t second_of_day = (int64_t)*timer % 86400;
    if (second_of_day < 0) {
        --days;
        second_of_day += 86400;
    }

    /* Gregorian civil_from_days, with epoch 1970-01-01. */
    int64_t z = days + 719468;
    int64_t era = (z >= 0 ? z : z - 146096) / 146097;
    unsigned doe = (unsigned)(z - era * 146097);
    unsigned yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    int64_t year = (int64_t)yoe + era * 400;
    unsigned doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    unsigned mp = (5 * doy + 2) / 153;
    unsigned day = doy - (153 * mp + 2) / 5 + 1;
    int month = (int)mp + (mp < 10 ? 3 : -9);
    year += month <= 2;
    if (year < 1 || year > 9999) return 0;

    static const unsigned days_before_month[12] = {
        0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334
    };
    int leap = (year % 4 == 0 && (year % 100 != 0 || year % 400 == 0));
    int weekday = (int)((days + 4) % 7);
    if (weekday < 0) weekday += 7;

    *result = (struct tm){0};
    result->tm_sec = (int)(second_of_day % 60);
    result->tm_min = (int)((second_of_day / 60) % 60);
    result->tm_hour = (int)(second_of_day / 3600);
    result->tm_mday = (int)day;
    result->tm_mon = (int)month - 1;
    result->tm_year = (int)year - 1900;
    result->tm_wday = weekday;
    result->tm_yday = (int)(days_before_month[month - 1] + day - 1 +
                            (leap && month > 2));
    result->tm_isdst = 0;
    result->tm_gmtoff = 0;
    result->tm_zone = "UTC";
    return result;
}

struct tm *localtime_r(const time_t *timer, struct tm *result)
{
    return gmtime_r(timer, result);
}

/* The target has one UTC timezone. This covers time's POSIX fallback path. */
struct tm *localtime(const time_t *timer)
{
    static struct tm calendar;
    return localtime_r(timer, &calendar);
}

void tzset(void) {}
