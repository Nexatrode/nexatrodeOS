#ifndef GHC_MICROKIT_SYS_TIMES_H
#define GHC_MICROKIT_SYS_TIMES_H

#include <time.h>
struct tms {
    clock_t tms_utime;
    clock_t tms_stime;
    clock_t tms_cutime;
    clock_t tms_cstime;
};
clock_t times(struct tms *buffer);

#endif
