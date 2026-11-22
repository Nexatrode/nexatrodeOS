#ifndef GHC_MICROKIT_SYS_TYPES_H
#define GHC_MICROKIT_SYS_TYPES_H

#include <stddef.h>

/* Compile-time POSIX integer types for GHC sources; not an OS service ABI. */
#ifndef ssize_t
typedef long ssize_t;
#endif
typedef int pid_t;
typedef long suseconds_t;
typedef unsigned int useconds_t;
typedef long off_t;
typedef unsigned int mode_t;
typedef unsigned long dev_t;
typedef unsigned long ino_t;
typedef unsigned long nlink_t;
typedef unsigned int uid_t;
typedef unsigned int gid_t;
typedef long blksize_t;
typedef long blkcnt_t;
typedef unsigned long nfds_t;

#endif
