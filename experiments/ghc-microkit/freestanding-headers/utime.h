#ifndef GHC_MICROKIT_UTIME_H
#define GHC_MICROKIT_UTIME_H

#include <time.h>
struct utimbuf {
    time_t actime;
    time_t modtime;
};
int utime(const char *path, const struct utimbuf *times);

#endif
