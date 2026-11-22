#ifndef GHC_MICROKIT_SYS_RESOURCE_H
#define GHC_MICROKIT_SYS_RESOURCE_H

#include <sys/time.h>
struct rusage {
    struct timeval ru_utime;
    struct timeval ru_stime;
    long ru_maxrss;
    long ru_ixrss;
    long ru_idrss;
    long ru_isrss;
    long ru_minflt;
    long ru_majflt;
    long ru_nswap;
    long ru_inblock;
    long ru_oublock;
    long ru_msgsnd;
    long ru_msgrcv;
    long ru_nsignals;
    long ru_nvcsw;
    long ru_nivcsw;
};
int getrusage(int who, struct rusage *usage);

#define RUSAGE_SELF 0

#endif
