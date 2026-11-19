#ifndef GHC_MICROKIT_UNISTD_H
#define GHC_MICROKIT_UNISTD_H

#include <stddef.h>
#include <sys/types.h>

#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define _SC_PAGESIZE 30

/* Compile-time ABI declarations only; Microkit implementations are pending. */
int close(int fd);
pid_t fork(void);
pid_t getpid(void);
pid_t getppid(void);
int isatty(int fd);
long lseek(int fd, long offset, int whence);
ssize_t read(int fd, void *buf, size_t count);
ssize_t write(int fd, const void *buf, size_t count);
long sysconf(int name);

#endif
