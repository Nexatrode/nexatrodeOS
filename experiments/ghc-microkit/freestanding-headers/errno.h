#ifndef GHC_MICROKIT_ERRNO_H
#define GHC_MICROKIT_ERRNO_H

/* Minimal freestanding errno surface for compiling GHC's non-threaded RTS. */
extern int errno;

#define EPERM 1
#define ENOENT 2
#define EINTR 4
#define EIO 5
#define EBADF 9
#define EAGAIN 11
#define ENOMEM 12
#define EACCES 13
#define EFAULT 14
#define EBUSY 16
#define EEXIST 17
#define EINVAL 22
#define ENOTTY 25
#define ENOSPC 28
#define ESPIPE 29
#define EPIPE 32
#define EDEADLK 35
#define ENOSYS 38
#define ETIMEDOUT 60

#endif
