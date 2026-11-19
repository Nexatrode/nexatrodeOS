
#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>
#include <sys/types.h>
long sysconf(int);
ssize_t read(int, void *, size_t);
ssize_t write(int, const void *, size_t);
off_t lseek(int, off_t, int);
int isatty(int);
int close(int);
int access(const char *, int);
int fsync(int);
ssize_t pread(int, void *, size_t, off_t);
ssize_t pwrite(int, const void *, size_t, off_t);
#define _SC_PAGESIZE 30
#define _SC_NPROCESSORS_ONLN 84
#define F_OK 0
#define R_OK 4
#define W_OK 2
#define X_OK 1

#ifdef __cplusplus
}
#endif
