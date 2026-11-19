#ifndef GHC_MICROKIT_SYS_SELECT_H
#define GHC_MICROKIT_SYS_SELECT_H

#include <sys/time.h>
typedef struct { unsigned long bits[16]; } fd_set;
int select(int nfds, fd_set *readfds, fd_set *writefds, fd_set *exceptfds,
           struct timeval *timeout);

#define FD_SETSIZE 1024
#define FD_ZERO(set) __builtin_memset((set), 0, sizeof(*(set)))
#define FD_SET(fd, set) ((set)->bits[(fd) / (8 * sizeof(unsigned long))] |= \
    1UL << ((fd) % (8 * sizeof(unsigned long))))
#define FD_CLR(fd, set) ((set)->bits[(fd) / (8 * sizeof(unsigned long))] &= \
    ~(1UL << ((fd) % (8 * sizeof(unsigned long))))
#define FD_ISSET(fd, set) (((set)->bits[(fd) / (8 * sizeof(unsigned long))] >> \
    ((fd) % (8 * sizeof(unsigned long))) & 1UL)

#endif
