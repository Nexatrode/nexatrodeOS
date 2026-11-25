/* SPDX-License-Identifier: GPL-2.0-only
 * Single-threaded offline C runtime hooks. No host, disk or network backend.
 */
#include <microkit.h>
#include <errno.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <fcntl.h>
#include <string.h>
#include <time.h>
#include <dirent.h>
#include "native.h"
#include <nsutils/time.h>
uintptr_t browser_heap;
int isascii(int c) { return (unsigned)c < 128; }
static size_t heap_used;
static uint64_t origin, frequency;

uint64_t native_ms(void) {
    uint64_t ticks;
    __asm__ volatile("mrs %0, cntpct_el0" : "=r"(ticks));
    if (!frequency) {
        __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(frequency));
        origin = ticks;
    }
    return (ticks - origin) * 1000 / frequency;
}
void *sbrk(ptrdiff_t amount) {
    const size_t capacity = 64u * 1024 * 1024;
    if (amount < 0 || (size_t)amount > capacity - heap_used) { errno = ENOMEM; return (void *)-1; }
    void *result = (void *)(browser_heap + heap_used);
    heap_used += (size_t)amount;
    return result;
}
static int serial_put(char c, FILE *f) { (void)f; microkit_dbg_putc(c); return 0; }
static int serial_get(FILE *f) { (void)f; return _FDEV_EOF; }
static FILE console = FDEV_SETUP_STREAM(serial_put, serial_get, NULL, _FDEV_SETUP_RW);
FILE *const stdin = &console;
FILE *const stdout = &console;
FILE *const stderr = &console;
void native_fail(const char *where, int error) {
    printf("NETSURF|FAIL|%s|%d\n", where, error);
    __builtin_trap();
}
void _exit(int status) { native_fail("exit", status); for (;;) {} }
nsuerror nsu_getmonotonic_ms(uint64_t *out) {
    *out = native_ms() + 1; /* A nonzero origin also enables the JS timeout on its first call. */
    return NSUERROR_OK;
}
static uint64_t unix_ms(void) {
    uint64_t now = native_ms();
#ifdef NATIVE_HTTPS
    static uint64_t cached, sampled;
    if (!cached || now - sampled >= 1000) {
        microkit_msginfo reply = microkit_ppcall(4,microkit_msginfo_new(0x205,0));
        if (microkit_msginfo_get_label(reply)==0x100 && microkit_msginfo_get_count(reply)==1) {
            uint64_t seconds=microkit_mr_get(0);
            if (seconds>=1735689600) {cached=seconds*1000;sampled=now;}
        }
    }
    if (cached) return cached + now - sampled;
#endif
    return 1700000000000ull + now; /* Only the offline fixture has no RTC service. */
}
double native_js_date_ms(void) { return (double)unix_ms(); }
double native_js_mono_ms(void) { return (double)native_ms(); }
int gettimeofday(struct timeval *tv, void *zone) {
    (void)zone; uint64_t ms = unix_ms();
    tv->tv_sec = ms / 1000; tv->tv_usec = ms % 1000 * 1000; return 0;
}
int clock_gettime(clockid_t id, struct timespec *ts) {
    (void)id; uint64_t ms = native_ms();
    ts->tv_sec = ms / 1000; ts->tv_nsec = ms % 1000 * 1000000; return 0;
}
time_t time(time_t *out) { time_t value = unix_ms()/1000; if (out) *out = value; return value; }
int open(const char *p, int flags, ...) { (void)p; (void)flags; errno = EACCES; return -1; }
int close(int fd) { (void)fd; errno = EBADF; return -1; }
ssize_t read(int fd, void *p, size_t n) { (void)fd; (void)p; (void)n; errno = EBADF; return -1; }
ssize_t write(int fd, const void *p, size_t n) {
    if (fd != 1 && fd != 2) { errno = EBADF; return -1; }
    for (size_t i = 0; i < n; ++i) microkit_dbg_putc(((const char *)p)[i]); return n;
}
off_t lseek(int fd, off_t off, int whence) { (void)fd; (void)off; (void)whence; errno = EBADF; return -1; }
int stat(const char *p, struct stat *s) { (void)p; (void)s; errno = ENOENT; return -1; }
int fstat(int fd, struct stat *s) { (void)fd; (void)s; errno = EBADF; return -1; }
int mkdir(const char *p, mode_t mode) { (void)p; (void)mode; errno = EACCES; return -1; }
int unlink(const char *p) { (void)p; errno = EACCES; return -1; }
int rename(const char *a, const char *b) { (void)a; (void)b; errno = EACCES; return -1; }
char *realpath(const char *a, char *b) { (void)a; (void)b; errno = EACCES; return NULL; }
char *getcwd(char *a, size_t n) { (void)a; (void)n; errno = EACCES; return NULL; }
int socket(int a, int b, int c) { (void)a; (void)b; (void)c; errno = ENOSYS; return -1; }
int inet_pton(int family, const char *s, void *d) { (void)family; (void)s; (void)d; return 0; }
int inet_aton(const char *s, void *d) { (void)s; (void)d; return 0; }
const struct asset *native_asset(const char *path) {
    for (size_t i = 0; i < native_asset_count; ++i)
        if (strcmp(native_assets[i].name, path) == 0) return &native_assets[i];
    return NULL;
}
