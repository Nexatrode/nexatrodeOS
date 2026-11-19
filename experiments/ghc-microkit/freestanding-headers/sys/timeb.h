#ifndef GHC_MICROKIT_SYS_TIMEB_H
#define GHC_MICROKIT_SYS_TIMEB_H

#include <time.h>
struct timeb {
    time_t time;
    unsigned short millitm;
    short timezone;
    short dstflag;
};
int ftime(struct timeb *time_buffer);

#endif
