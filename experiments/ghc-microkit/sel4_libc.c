/*
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Bounded, freestanding runtime primitives for the initial non-threaded GHC
 * image. The allocator is single-threaded and intentionally independent from
 * GHC's separately managed 1 MiB megablock arena.
 */
#include "sel4_libc.h"

#include <errno.h>
#include <fcntl.h>
#include <microkit.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/stat.h>
#include <utime.h>
#include <unistd.h>

#ifndef EOVERFLOW
#define EOVERFLOW 75
#endif
#ifndef EISDIR
#define EISDIR 21
#endif
#ifndef ENOTDIR
#define ENOTDIR 20
#endif
#ifndef EMFILE
#define EMFILE 24
#endif

/* Microkit protection domains start with no inherited environment. */
int errno;
char *sel4_empty_environment[] = { NULL };
char **environ = sel4_empty_environment;

int abs(int value)
{
    return value < 0 ? -value : value;
}

/* This image uses GHC's non-threaded RTS; creating an OS thread is unsupported. */
int forkOS_createThread(void *entry)
{
    (void)entry;
    return -1;
}

static int fail_with(int error)
{
    errno = error;
    return -1;
}

/*
 * A Microkit PD is not a hosted process. Successful application shutdown
 * records its status and parks this component; fatal termination faults the
 * PD so the supervisor can observe the failure.
 */
_Noreturn void exit(int status)
{
    microkit_dbg_puts("HASKELL_EXIT|");
    microkit_dbg_put32((uint32_t)status);
    microkit_dbg_putc('\n');
    for (;;) {
        __asm__ volatile("wfe" ::: "memory");
    }
}

_Noreturn void abort(void)
{
    microkit_dbg_puts("HASKELL_ABORT\n");
    __builtin_trap();
    __builtin_unreachable();
}

char *strerror(int error)
{
    switch (error) {
    case 0: return "Success";
    case EPERM: return "Operation not permitted";
    case ENOENT: return "No such file or directory";
    case EINTR: return "Interrupted system call";
    case EIO: return "Input/output error";
    case EBADF: return "Bad file descriptor";
    case EAGAIN: return "Resource temporarily unavailable";
    case ENOMEM: return "Not enough memory";
    case EACCES: return "Permission denied";
    case EFAULT: return "Bad address";
    case EBUSY: return "Device or resource busy";
    case EEXIST: return "File exists";
    case EINVAL: return "Invalid argument";
    case ENOTTY: return "Inappropriate ioctl for device";
    case ENOSPC: return "No space left on device";
    case ESPIPE: return "Illegal seek";
    case EPIPE: return "Broken pipe";
    case EDEADLK: return "Resource deadlock avoided";
    case ENOSYS: return "Function not implemented";
    case ETIMEDOUT: return "Operation timed out";
    default: return "Unknown error";
    }
}

/*
 * Bounded in-PD ramfs for XFTP work files under "xftp/". SQLite still uses its
 * own VFS; this only backs open/read/write/mkdir for the SimpleX file agent.
 */
#define RAMFS_MAX_NODES 48
#define RAMFS_MAX_NAME 160
#define RAMFS_MAX_FDS 24
#define RAMFS_FD_BASE 32
// XFTP work files include encryption/framing overhead beyond the 1 MiB
// attachment limit. The ramfs grows each file on demand within this bound.
#define RAMFS_FILE_CAP (2u * 1024u * 1024u)

typedef struct {
    int used;
    int is_dir;
    char name[RAMFS_MAX_NAME];
    size_t size;
    size_t cap;
    unsigned char *data;
} ramfs_node_t;

typedef struct {
    int used;
    int node;
    size_t pos;
    int writable;
} ramfs_fd_t;

static ramfs_node_t ramfs_nodes[RAMFS_MAX_NODES];
static ramfs_fd_t ramfs_fds[RAMFS_MAX_FDS];

static int ramfs_path_ok(const char *path)
{
    return path != NULL && path[0] != '\0' && strnlen(path, RAMFS_MAX_NAME) < RAMFS_MAX_NAME;
}

static void ramfs_normalize(char *out, const char *path)
{
    const char *src = path;
    size_t n = 0;
    if (src[0] == '.' && src[1] == '/') {
        src += 2;
    }
    while (*src != '\0' && n + 1 < RAMFS_MAX_NAME) {
        if (src[0] == '/' && (n == 0 || out[n - 1] == '/')) {
            ++src;
            continue;
        }
        out[n++] = *src++;
    }
    while (n > 1 && out[n - 1] == '/') {
        --n;
    }
    out[n] = '\0';
}

static int ramfs_find(const char *path)
{
    char normalized[RAMFS_MAX_NAME];
    if (!ramfs_path_ok(path)) {
        return -1;
    }
    ramfs_normalize(normalized, path);
    for (int i = 0; i < RAMFS_MAX_NODES; ++i) {
        if (ramfs_nodes[i].used && strcmp(ramfs_nodes[i].name, normalized) == 0) {
            return i;
        }
    }
    return -1;
}

static int ramfs_alloc_node(const char *path, int is_dir)
{
    char normalized[RAMFS_MAX_NAME];
    if (!ramfs_path_ok(path)) {
        return -1;
    }
    ramfs_normalize(normalized, path);
    if (normalized[0] == '\0') {
        return -1;
    }
    for (int i = 0; i < RAMFS_MAX_NODES; ++i) {
        if (!ramfs_nodes[i].used) {
            ramfs_nodes[i].used = 1;
            ramfs_nodes[i].is_dir = is_dir;
            ramfs_nodes[i].size = 0;
            ramfs_nodes[i].cap = 0;
            ramfs_nodes[i].data = NULL;
            strncpy(ramfs_nodes[i].name, normalized, RAMFS_MAX_NAME - 1);
            ramfs_nodes[i].name[RAMFS_MAX_NAME - 1] = '\0';
            return i;
        }
    }
    return -1;
}

static int ramfs_fd_from(int fd)
{
    if (fd < RAMFS_FD_BASE || fd >= RAMFS_FD_BASE + RAMFS_MAX_FDS) {
        return -1;
    }
    int slot = fd - RAMFS_FD_BASE;
    return ramfs_fds[slot].used ? slot : -1;
}

static int ramfs_open_fd(int node, int writable)
{
    for (int i = 0; i < RAMFS_MAX_FDS; ++i) {
        if (!ramfs_fds[i].used) {
            ramfs_fds[i].used = 1;
            ramfs_fds[i].node = node;
            ramfs_fds[i].pos = 0;
            ramfs_fds[i].writable = writable;
            return RAMFS_FD_BASE + i;
        }
    }
    return -1;
}

/*
 * The debug console is the first stdout/stderr device. It is a byte sink,
 * not a terminal: there is no cursor, line discipline, or keyboard input.
 */
ssize_t write(int fd, const void *buffer, size_t count)
{
    int slot = ramfs_fd_from(fd);
    if (slot >= 0) {
        ramfs_fd_t *handle = &ramfs_fds[slot];
        ramfs_node_t *node = &ramfs_nodes[handle->node];
        if (!handle->writable || node->is_dir) {
            return fail_with(EBADF);
        }
        if (buffer == NULL && count != 0) {
            return fail_with(EFAULT);
        }
        size_t end = handle->pos + count;
        if (end > RAMFS_FILE_CAP) {
            return fail_with(ENOSPC);
        }
        if (end > node->cap) {
            size_t next = node->cap == 0 ? 4096 : node->cap;
            while (next < end) {
                next *= 2;
                if (next > RAMFS_FILE_CAP) {
                    next = RAMFS_FILE_CAP;
                    break;
                }
            }
            unsigned char *grown = realloc(node->data, next);
            if (grown == NULL) {
                return fail_with(ENOMEM);
            }
            node->data = grown;
            node->cap = next;
        }
        memcpy(node->data + handle->pos, buffer, count);
        handle->pos = end;
        if (end > node->size) {
            node->size = end;
        }
        return (ssize_t)count;
    }
    if (fd != STDOUT_FILENO && fd != STDERR_FILENO) {
        return fail_with(EBADF);
    }
    if (buffer == NULL && count != 0) {
        return fail_with(EFAULT);
    }
    const unsigned char *bytes = buffer;
    for (size_t i = 0; i < count; ++i) {
        microkit_dbg_putc(bytes[i]);
    }
    return (ssize_t)count;
}

ssize_t read(int fd, void *buffer, size_t count)
{
    int slot = ramfs_fd_from(fd);
    if (slot >= 0) {
        ramfs_fd_t *handle = &ramfs_fds[slot];
        ramfs_node_t *node = &ramfs_nodes[handle->node];
        if (node->is_dir) {
            return fail_with(EISDIR);
        }
        if (buffer == NULL && count != 0) {
            return fail_with(EFAULT);
        }
        if (handle->pos >= node->size) {
            return 0;
        }
        size_t available = node->size - handle->pos;
        size_t take = count < available ? count : available;
        memcpy(buffer, node->data + handle->pos, take);
        handle->pos += take;
        return (ssize_t)take;
    }
    if (fd != STDIN_FILENO) {
        return fail_with(EBADF);
    }
    (void)buffer;
    (void)count;
    return fail_with(ENOSYS); /* No input capability is wired to this PD. */
}

int close(int fd)
{
    int slot = ramfs_fd_from(fd);
    if (slot >= 0) {
        ramfs_fds[slot].used = 0;
        ramfs_fds[slot].node = -1;
        ramfs_fds[slot].pos = 0;
        ramfs_fds[slot].writable = 0;
        return 0;
    }
    if (fd >= STDIN_FILENO && fd <= STDERR_FILENO) {
        return fail_with(ENOSYS); /* No descriptor table or close service yet. */
    }
    return fail_with(EBADF);
}

int isatty(int fd)
{
    if (ramfs_fd_from(fd) >= 0) {
        errno = ENOTTY;
        return 0;
    }
    if (fd < STDIN_FILENO || fd > STDERR_FILENO) {
        errno = EBADF;
    } else {
        errno = ENOTTY;
    }
    return 0;
}

int fstat(int fd, struct stat *status)
{
    int slot = ramfs_fd_from(fd);
    if (slot >= 0) {
        if (status == NULL) {
            return fail_with(EFAULT);
        }
        int node_index = ramfs_fds[slot].node;
        ramfs_node_t *node = &ramfs_nodes[node_index];
        *status = (struct stat){0};
        /* Distinct (dev,ino) so GHC's lockFile does not treat every ramfs
         * path as the same RegularFile. */
        status->st_dev = 1;
        status->st_ino = (ino_t)(node_index + 1);
        status->st_mode = (node->is_dir ? S_IFDIR : S_IFREG) | 0700;
        status->st_nlink = 1;
        status->st_size = (off_t)node->size;
        return 0;
    }
    if (fd < STDIN_FILENO || fd > STDERR_FILENO) {
        return fail_with(EBADF);
    }
    if (status == NULL) {
        return fail_with(EFAULT);
    }
    *status = (struct stat){0};
    status->st_mode = S_IFIFO | (fd == STDIN_FILENO ? S_IRUSR : S_IWUSR);
    status->st_nlink = 1;
    return 0;
}

long lseek(int fd, long offset, int whence)
{
    int slot = ramfs_fd_from(fd);
    if (slot >= 0) {
        ramfs_fd_t *handle = &ramfs_fds[slot];
        ramfs_node_t *node = &ramfs_nodes[handle->node];
        long next = 0;
        switch (whence) {
        case SEEK_SET:
            next = offset;
            break;
        case SEEK_CUR:
            next = (long)handle->pos + offset;
            break;
        case SEEK_END:
            next = (long)node->size + offset;
            break;
        default:
            return fail_with(EINVAL);
        }
        if (next < 0) {
            return fail_with(EINVAL);
        }
        handle->pos = (size_t)next;
        return next;
    }
    (void)offset;
    (void)whence;
    return fail_with(fd >= STDIN_FILENO && fd <= STDERR_FILENO ? ESPIPE : EBADF);
}

char *getenv(const char *name)
{
    (void)name;
    return NULL;
}

int putenv(char *entry)
{
    (void)entry;
    return fail_with(ENOSYS); /* A PD has no mutable process environment. */
}

int open(const char *path, int flags, ...)
{
    int node = ramfs_find(path);
    int create = (flags & O_CREAT) != 0;
    int trunc = (flags & O_TRUNC) != 0;
    int writable = (flags & O_WRONLY) != 0 || (flags & O_RDWR) != 0;
    if (node < 0) {
        if (!create) {
            return fail_with(ENOENT);
        }
        node = ramfs_alloc_node(path, 0);
        if (node < 0) {
            return fail_with(ENOSPC);
        }
    } else if (ramfs_nodes[node].is_dir) {
        return fail_with(EISDIR);
    } else if ((flags & O_EXCL) != 0 && create) {
        return fail_with(EEXIST);
    }
    if (trunc && writable) {
        ramfs_nodes[node].size = 0;
    }
    int fd = ramfs_open_fd(node, writable || create);
    if (fd < 0) {
        return fail_with(EMFILE);
    }
    if ((flags & O_APPEND) != 0) {
        ramfs_fds[fd - RAMFS_FD_BASE].pos = ramfs_nodes[node].size;
    }
    return fd;
}

int access(const char *path, int mode)
{
    int node = ramfs_find(path);
    if (node < 0) {
        return fail_with(ENOENT);
    }
    (void)mode;
    return 0;
}

int creat(const char *path, mode_t mode)
{
    (void)mode;
    return open(path, O_CREAT | O_WRONLY | O_TRUNC, 0600);
}

int stat(const char *path, struct stat *status)
{
    int node = ramfs_find(path);
    if (node < 0) {
        return fail_with(ENOENT);
    }
    if (status == NULL) {
        return fail_with(EFAULT);
    }
    *status = (struct stat){0};
    status->st_dev = 1;
    status->st_ino = (ino_t)(node + 1);
    status->st_mode = (ramfs_nodes[node].is_dir ? S_IFDIR : S_IFREG) | 0700;
    status->st_nlink = 1;
    status->st_size = (off_t)ramfs_nodes[node].size;
    return 0;
}

int lstat(const char *path, struct stat *status)
{
    return stat(path, status);
}

int mkdir(const char *path, mode_t mode)
{
    (void)mode;
    if (ramfs_find(path) >= 0) {
        return fail_with(EEXIST);
    }
    if (ramfs_alloc_node(path, 1) < 0) {
        return fail_with(ENOSPC);
    }
    return 0;
}

int unlink(const char *path)
{
    int node = ramfs_find(path);
    if (node < 0) {
        return fail_with(ENOENT);
    }
    if (ramfs_nodes[node].is_dir) {
        return fail_with(EISDIR);
    }
    for (int i = 0; i < RAMFS_MAX_FDS; ++i) {
        if (ramfs_fds[i].used && ramfs_fds[i].node == node) {
            return fail_with(EBUSY);
        }
    }
    free(ramfs_nodes[node].data);
    ramfs_nodes[node].data = NULL;
    ramfs_nodes[node].used = 0;
    ramfs_nodes[node].size = 0;
    ramfs_nodes[node].cap = 0;
    ramfs_nodes[node].name[0] = '\0';
    return 0;
}

/* Helpers for the Haskell Directory shim (no full POSIX stat binding). */
long sel4_path_size(const char *path)
{
    int node = ramfs_find(path);
    if (node < 0) {
        errno = ENOENT;
        return -1;
    }
    return (long)ramfs_nodes[node].size;
}

int sel4_path_is_dir(const char *path)
{
    int node = ramfs_find(path);
    if (node < 0) {
        errno = ENOENT;
        return -1;
    }
    return ramfs_nodes[node].is_dir ? 1 : 0;
}

int fcntl(int fd, int command, ...)
{
    /*
     * GHC openBinaryFile / withBinaryFile sets FD_CLOEXEC and probes O_NONBLOCK
     * via fcntl after open. Ramfs has no process table — accept the usual
     * get/set flag commands as no-ops so XFTP work files can be created.
     */
    int slot = ramfs_fd_from(fd);
    if (slot < 0 && (fd < STDIN_FILENO || fd > STDERR_FILENO)) {
        return fail_with(EBADF);
    }
    switch (command) {
    case F_GETFD:
        return 0;
    case F_SETFD:
        return 0;
    case F_GETFL:
        if (slot >= 0) {
            return ramfs_fds[slot].writable ? O_RDWR : O_RDONLY;
        }
        return (fd == STDIN_FILENO) ? O_RDONLY : O_WRONLY;
    case F_SETFL:
        return 0;
    default:
        return fail_with(ENOSYS);
    }
}

int ftruncate(int fd, off_t length)
{
    int slot = ramfs_fd_from(fd);
    if (slot < 0) {
        return fail_with(EBADF);
    }
    if (length < 0) {
        return fail_with(EINVAL);
    }
    ramfs_fd_t *handle = &ramfs_fds[slot];
    ramfs_node_t *node = &ramfs_nodes[handle->node];
    if (node->is_dir || !handle->writable) {
        return fail_with(EBADF);
    }
    size_t want = (size_t)length;
    if (want > RAMFS_FILE_CAP) {
        return fail_with(ENOSPC);
    }
    if (want > node->cap) {
        unsigned char *grown = realloc(node->data, want == 0 ? 1 : want);
        if (grown == NULL && want != 0) {
            return fail_with(ENOMEM);
        }
        node->data = grown;
        node->cap = want;
    }
    if (want > node->size && node->data != NULL) {
        memset(node->data + node->size, 0, want - node->size);
    }
    node->size = want;
    if (handle->pos > want) {
        handle->pos = want;
    }
    return 0;
}

/* Older base builds may omit this wrapper. Current base defines it and calls
 * ftruncate itself, so keep this fallback weak to avoid a duplicate symbol. */
__attribute__((weak)) int __hscore_ftruncate(int fd, off_t where)
{
    return ftruncate(fd, where);
}

int link(const char *old_path, const char *new_path)
{
    (void)old_path;
    (void)new_path;
    return fail_with(ENOSYS);
}

mode_t umask(mode_t mask)
{
    static mode_t current_mask = 0022;
    mode_t old_mask = current_mask;
    current_mask = mask & 0777;
    return old_mask;
}

int utime(const char *path, const struct utimbuf *times)
{
    (void)path;
    (void)times;
    return fail_with(ENOSYS);
}

/*
 * Diagnostic-only stdio: stdin has no input capability, and stdout/stderr
 * are synchronous debug-console streams. FILE objects are identity tokens;
 * this deliberately does not imply a filesystem or buffered I/O layer.
 */
static unsigned char stdin_token, stdout_token, stderr_token;
FILE *stdin = (FILE *)&stdin_token;
FILE *stdout = (FILE *)&stdout_token;
FILE *stderr = (FILE *)&stderr_token;

typedef struct format_output {
    char *buffer;
    size_t capacity;
    size_t length;
    int failed;
} format_output_t;

static void format_char(format_output_t *out, char c)
{
    if (out->length == SIZE_MAX) {
        out->failed = 1;
        return;
    }
    if (out->buffer != NULL && out->capacity != 0 && out->length < out->capacity - 1u) {
        out->buffer[out->length] = c;
    }
    ++out->length;
}

static void format_repeat(format_output_t *out, char c, size_t count)
{
    while (count-- != 0) format_char(out, c);
}

enum format_length { LEN_DEFAULT, LEN_HH, LEN_H, LEN_L, LEN_LL, LEN_Z, LEN_T, LEN_J };

static uintmax_t format_unsigned(__builtin_va_list *args, enum format_length length)
{
    switch (length) {
    case LEN_L: return __builtin_va_arg(*args, unsigned long);
    case LEN_LL: return __builtin_va_arg(*args, unsigned long long);
    case LEN_Z: return __builtin_va_arg(*args, size_t);
    case LEN_T: return __builtin_va_arg(*args, uintptr_t);
    case LEN_J: return __builtin_va_arg(*args, uintmax_t);
    case LEN_HH: return (unsigned char)__builtin_va_arg(*args, int);
    case LEN_H: return (unsigned short)__builtin_va_arg(*args, int);
    default: return __builtin_va_arg(*args, unsigned int);
    }
}

static intmax_t format_signed(__builtin_va_list *args, enum format_length length)
{
    switch (length) {
    case LEN_L: return __builtin_va_arg(*args, long);
    case LEN_LL: return __builtin_va_arg(*args, long long);
    case LEN_Z: return (intmax_t)__builtin_va_arg(*args, ptrdiff_t);
    case LEN_T: return __builtin_va_arg(*args, ptrdiff_t);
    case LEN_J: return __builtin_va_arg(*args, intmax_t);
    case LEN_HH: return (signed char)__builtin_va_arg(*args, int);
    case LEN_H: return (short)__builtin_va_arg(*args, int);
    default: return __builtin_va_arg(*args, int);
    }
}

static int format_v(format_output_t *out, const char *format, __builtin_va_list args)
{
    while (*format != '\0' && !out->failed) {
        if (*format != '%') {
            format_char(out, *format++);
            continue;
        }
        ++format;
        if (*format == '%') {
            format_char(out, *format++);
            continue;
        }

        int left = 0, plus = 0, space = 0, alternate = 0, zero = 0;
        for (;;) {
            if (*format == '-') left = 1;
            else if (*format == '+') plus = 1;
            else if (*format == ' ') space = 1;
            else if (*format == '#') alternate = 1;
            else if (*format == '0') zero = 1;
            else break;
            ++format;
        }
        size_t width = 0;
        if (*format == '*') {
            int w = __builtin_va_arg(args, int);
            ++format;
            if (w < 0) { left = 1; width = (size_t)(-(intmax_t)w); }
            else width = (size_t)w;
        } else {
            while (*format >= '0' && *format <= '9') {
                unsigned digit = (unsigned)(*format++ - '0');
                if (width > (SIZE_MAX - digit) / 10u) { out->failed = 1; break; }
                width = width * 10u + digit;
            }
        }
        int precision_set = 0;
        size_t precision = 0;
        if (*format == '.') {
            ++format;
            precision_set = 1;
            if (*format == '*') {
                int p = __builtin_va_arg(args, int);
                ++format;
                if (p < 0) precision_set = 0;
                else precision = (size_t)p;
            } else {
                while (*format >= '0' && *format <= '9') {
                    unsigned digit = (unsigned)(*format++ - '0');
                    if (precision > (SIZE_MAX - digit) / 10u) { out->failed = 1; break; }
                    precision = precision * 10u + digit;
                }
            }
        }
        enum format_length length = LEN_DEFAULT;
        if (*format == 'h') { ++format; length = (*format == 'h') ? (++format, LEN_HH) : LEN_H; }
        else if (*format == 'l') { ++format; length = (*format == 'l') ? (++format, LEN_LL) : LEN_L; }
        else if (*format == 'z') { ++format; length = LEN_Z; }
        else if (*format == 't') { ++format; length = LEN_T; }
        else if (*format == 'j') { ++format; length = LEN_J; }

        char conversion = *format;
        if (conversion == '\0') { out->failed = 1; break; }
        ++format;
        if (conversion == 's') {
            const char *s = __builtin_va_arg(args, const char *);
            if (s == NULL) s = "(null)";
            size_t n = precision_set ? strnlen(s, precision) : strlen(s);
            size_t pad = width > n ? width - n : 0;
            if (!left) format_repeat(out, ' ', pad);
            for (size_t i = 0; i < n; ++i) format_char(out, s[i]);
            if (left) format_repeat(out, ' ', pad);
            continue;
        }
        if (conversion == 'c') {
            char c = (char)__builtin_va_arg(args, int);
            size_t pad = width > 1u ? width - 1u : 0;
            if (!left) format_repeat(out, ' ', pad);
            format_char(out, c);
            if (left) format_repeat(out, ' ', pad);
            continue;
        }
        if (conversion == 'd' || conversion == 'i' || conversion == 'u' ||
            conversion == 'o' || conversion == 'x' || conversion == 'X' || conversion == 'p') {
            unsigned base = conversion == 'o' ? 8u :
                            (conversion == 'x' || conversion == 'X' || conversion == 'p') ? 16u : 10u;
            int is_signed = conversion == 'd' || conversion == 'i';
            int negative = 0;
            uintmax_t value;
            if (conversion == 'p') value = (uintptr_t)__builtin_va_arg(args, void *);
            else if (is_signed) {
                intmax_t signed_value = format_signed(&args, length);
                negative = signed_value < 0;
                value = negative ? (uintmax_t)(-(signed_value + 1)) + 1u : (uintmax_t)signed_value;
            } else value = format_unsigned(&args, length);
            char digits[sizeof(uintmax_t) * CHAR_BIT + 1u];
            size_t digit_count = 0;
            if (value != 0 || !precision_set || precision != 0) {
                do {
                    unsigned digit = (unsigned)(value % base);
                    digits[digit_count++] = (char)(digit < 10u ? '0' + digit :
                                            (conversion == 'X' ? 'A' : 'a') + digit - 10u);
                    value /= base;
                } while (value != 0);
            }
            size_t leading_zeroes = precision_set && precision > digit_count ? precision - digit_count : 0;
            char sign = negative ? '-' : (plus && is_signed) ? '+' : (space && is_signed) ? ' ' : '\0';
            char prefix0 = '\0', prefix1 = '\0';
            if (conversion == 'p' || (alternate && digit_count != 0 && base == 16u)) {
                prefix0 = '0'; prefix1 = conversion == 'X' ? 'X' : 'x';
            } else if (alternate && base == 8u && (digit_count == 0 || digits[digit_count - 1u] != '0')) {
                prefix0 = '0';
            }
            size_t prefix_len = (prefix0 != '\0') + (prefix1 != '\0');
            size_t core = digit_count + leading_zeroes + prefix_len + (sign != '\0');
            size_t pad = width > core ? width - core : 0;
            int use_zero_width = zero && !left && !precision_set;
            if (!left && !use_zero_width) format_repeat(out, ' ', pad);
            if (sign != '\0') format_char(out, sign);
            if (prefix0 != '\0') format_char(out, prefix0);
            if (prefix1 != '\0') format_char(out, prefix1);
            if (use_zero_width) format_repeat(out, '0', pad);
            format_repeat(out, '0', leading_zeroes);
            while (digit_count != 0) format_char(out, digits[--digit_count]);
            if (left) format_repeat(out, ' ', pad);
            continue;
        }
        /* Floating point and extensions such as %n are not part of this ABI. */
        out->failed = 1;
    }
    if (out->failed || out->length > (size_t)INT_MAX) return -1;
    return (int)out->length;
}

int vsnprintf(char *buffer, size_t size, const char *format, __builtin_va_list args)
{
    if (format == NULL || (buffer == NULL && size != 0)) return fail_with(EINVAL);
    format_output_t out = { .buffer = buffer, .capacity = size, .length = 0, .failed = 0 };
    int result = format_v(&out, format, args);
    if (size != 0) buffer[out.length < size - 1u ? out.length : size - 1u] = '\0';
    if (result < 0) return fail_with(EINVAL);
    return result;
}

int snprintf(char *buffer, size_t size, const char *format, ...)
{
    __builtin_va_list args;
    __builtin_va_start(args, format);
    int result = vsnprintf(buffer, size, format, args);
    __builtin_va_end(args);
    return result;
}

int sprintf(char *buffer, const char *format, ...)
{
    __builtin_va_list args;
    __builtin_va_start(args, format);
    int result = vsnprintf(buffer, SIZE_MAX, format, args);
    __builtin_va_end(args);
    return result;
}

static int stream_fd(FILE *stream)
{
    if (stream == stdout) return STDOUT_FILENO;
    if (stream == stderr) return STDERR_FILENO;
    return -1;
}

int vfprintf(FILE *stream, const char *format, __builtin_va_list args)
{
    int fd = stream_fd(stream);
    if (fd < 0) return fail_with(stream == stdin ? EBADF : ENOSYS);
    char buffer[1024];
    int length = vsnprintf(buffer, sizeof(buffer), format, args);
    if (length < 0) return -1;
    if ((size_t)length >= sizeof(buffer)) return fail_with(EOVERFLOW);
    if (write(fd, buffer, (size_t)length) != (ssize_t)length) return -1;
    return length;
}

int fprintf(FILE *stream, const char *format, ...)
{
    __builtin_va_list args;
    __builtin_va_start(args, format);
    int result = vfprintf(stream, format, args);
    __builtin_va_end(args);
    return result;
}

int printf(const char *format, ...)
{
    __builtin_va_list args;
    __builtin_va_start(args, format);
    int result = vfprintf(stdout, format, args);
    __builtin_va_end(args);
    return result;
}

size_t fwrite(const void *ptr, size_t size, size_t count, FILE *stream)
{
    int fd = stream_fd(stream);
    if (fd < 0) { (void)fail_with(stream == stdin ? EBADF : ENOSYS); return 0; }
    if (size != 0 && count > SIZE_MAX / size) { (void)fail_with(EOVERFLOW); return 0; }
    size_t bytes = size * count;
    ssize_t written = write(fd, ptr, bytes);
    return written < 0 ? 0 : (size == 0 ? count : (size_t)written / size);
}

int fputc(int c, FILE *stream)
{
    unsigned char byte = (unsigned char)c;
    return fwrite(&byte, 1, 1, stream) == 1 ? byte : EOF;
}

int fputs(const char *s, FILE *stream)
{
    if (s == NULL) return fail_with(EINVAL);
    size_t length = strlen(s);
    return fwrite(s, 1, length, stream) == length ? 0 : EOF;
}

int puts(const char *s)
{
    if (fputs(s, stdout) == EOF || fputc('\n', stdout) == EOF) return EOF;
    return 0;
}

int fflush(FILE *stream)
{
    if (stream == NULL || stream == stdout || stream == stderr) return 0;
    return fail_with(stream == stdin ? ENOSYS : EBADF);
}

int getc(FILE *stream)
{
    if (stream != stdin) { (void)fail_with(EBADF); return EOF; }
    (void)fail_with(ENOSYS);
    return EOF;
}

size_t fread(void *ptr, size_t size, size_t count, FILE *stream)
{
    (void)ptr;
    if (stream != stdin) (void)fail_with(EBADF);
    else (void)fail_with(ENOSYS);
    (void)size;
    (void)count;
    return 0;
}

FILE *fopen(const char *path, const char *mode)
{
    (void)path;
    (void)mode;
    (void)fail_with(ENOSYS);
    return NULL;
}

int fclose(FILE *stream)
{
    (void)stream;
    return fail_with(ENOSYS);
}

#ifndef SEL4_LIBC_HEAP_SIZE
#define SEL4_LIBC_HEAP_SIZE (1024u * 1024u)
#endif

#define HEAP_ALIGNMENT 16u
#define BLOCK_MAGIC UINT64_C(0x53454c34424c4f43)
#define ALLOC_MAGIC UINT64_C(0x53454c34414c4c4f)
#define ALIGN_UP(value, alignment) (((value) + ((alignment) - 1u)) & ~((alignment) - 1u))

typedef struct block {
    size_t size;
    size_t previous_size;
    size_t requested_size;
    uint64_t magic;
    uint32_t is_free;
    uint32_t reserved;
} block_t;

static _Alignas(HEAP_ALIGNMENT) unsigned char heap[SEL4_LIBC_HEAP_SIZE];
static block_t *first_block;

static size_t header_size(void)
{
    return ALIGN_UP(sizeof(block_t), HEAP_ALIGNMENT);
}

static size_t prefix_size(void)
{
    return ALIGN_UP(sizeof(block_t *) + sizeof(uint64_t), HEAP_ALIGNMENT);
}

static int is_power_of_two(size_t value)
{
    return value != 0 && (value & (value - 1u)) == 0;
}

static void initialize_heap(void)
{
    if (first_block != NULL) {
        return;
    }
    const size_t start = ALIGN_UP((uintptr_t)heap, HEAP_ALIGNMENT);
    const size_t end = ((uintptr_t)heap + sizeof(heap)) & ~(HEAP_ALIGNMENT - 1u);
    if (end <= start || end - start <= header_size()) {
        return;
    }
    first_block = (block_t *)start;
    first_block->size = end - start;
    first_block->previous_size = 0;
    first_block->requested_size = 0;
    first_block->magic = BLOCK_MAGIC;
    first_block->is_free = 1;
    first_block->reserved = 0;
}

static void *allocate_aligned(size_t alignment, size_t size)
{
    if (!is_power_of_two(alignment) || alignment < sizeof(void *)) {
        return NULL;
    }
    if (size == 0) {
        size = 1;
    }
    const size_t prefix = prefix_size();
    if (size > SIZE_MAX - prefix || size + prefix > SIZE_MAX - (alignment - 1u)) {
        return NULL;
    }
    const size_t needed = ALIGN_UP(prefix + size + alignment - 1u, HEAP_ALIGNMENT);
    const size_t head = header_size();
    initialize_heap();

    for (block_t *b = first_block; b != NULL;) {
        if (b->magic != BLOCK_MAGIC || b->size < head ||
            b->size > (uintptr_t)heap + sizeof(heap) - (uintptr_t)b) {
            return NULL;
        }
        if (b->is_free && b->size >= head + needed) {
            const size_t remainder = b->size - head - needed;
            if (remainder >= head + HEAP_ALIGNMENT) {
                block_t *split = (block_t *)((unsigned char *)b + head + needed);
                split->size = remainder;
                split->previous_size = head + needed;
                split->requested_size = 0;
                split->magic = BLOCK_MAGIC;
                split->is_free = 1;
                split->reserved = 0;
                b->size = head + needed;
                block_t *after = (block_t *)((unsigned char *)split + split->size);
                if ((uintptr_t)after < (uintptr_t)heap + sizeof(heap)) {
                    after->previous_size = split->size;
                }
            }
            b->is_free = 0;
            b->requested_size = size;
            unsigned char *payload = (unsigned char *)b + head;
            uintptr_t result = ALIGN_UP((uintptr_t)payload + prefix, alignment);
            block_t **stored_block = (block_t **)(result - sizeof(uint64_t) - sizeof(block_t *));
            uint64_t *stored_magic = (uint64_t *)(result - sizeof(uint64_t));
            *stored_block = b;
            *stored_magic = ALLOC_MAGIC;
            return (void *)result;
        }
        if (b->size > (uintptr_t)heap + sizeof(heap) - (uintptr_t)b - head) {
            return NULL;
        }
        b = (block_t *)((unsigned char *)b + b->size);
        if ((uintptr_t)b >= (uintptr_t)heap + sizeof(heap)) {
            break;
        }
    }
    return NULL;
}

static block_t *get_block(void *ptr)
{
    if (ptr == NULL || (uintptr_t)ptr < (uintptr_t)heap + prefix_size()) {
        return NULL;
    }
    uint64_t *magic = (uint64_t *)((uintptr_t)ptr - sizeof(uint64_t));
    if (*magic != ALLOC_MAGIC) {
        return NULL;
    }
    block_t *b = *(block_t **)((uintptr_t)ptr - sizeof(uint64_t) - sizeof(block_t *));
    if ((uintptr_t)b < (uintptr_t)heap || (uintptr_t)b >= (uintptr_t)heap + sizeof(heap) ||
        b->magic != BLOCK_MAGIC || b->is_free) {
        return NULL;
    }
    return b;
}

static void release_block(block_t *b)
{
    b->is_free = 1;
    uintptr_t heap_end = (uintptr_t)heap + sizeof(heap);
    block_t *next = (block_t *)((unsigned char *)b + b->size);
    if ((uintptr_t)next < heap_end && next->magic == BLOCK_MAGIC && next->is_free) {
        b->size += next->size;
        block_t *after = (block_t *)((unsigned char *)b + b->size);
        if ((uintptr_t)after < heap_end) {
            after->previous_size = b->size;
        }
    }
    if (b->previous_size != 0 && b->previous_size <= (uintptr_t)b - (uintptr_t)heap) {
        block_t *prev = (block_t *)((unsigned char *)b - b->previous_size);
        if (prev->magic == BLOCK_MAGIC && prev->is_free) {
            prev->size += b->size;
            block_t *after = (block_t *)((unsigned char *)prev + prev->size);
            if ((uintptr_t)after < heap_end) {
                after->previous_size = prev->size;
            }
        }
    }
}

void *malloc(size_t size)
{
    return allocate_aligned(HEAP_ALIGNMENT, size);
}

void *calloc(size_t count, size_t size)
{
    if (count != 0 && size > SIZE_MAX / count) {
        return NULL;
    }
    size_t total = count * size;
    void *result = malloc(total);
    if (result != NULL) {
        (void)memset(result, 0, total);
    }
    return result;
}

void free(void *ptr)
{
    block_t *b = get_block(ptr);
    if (b == NULL) {
        return;
    }
    uint64_t *magic = (uint64_t *)((uintptr_t)ptr - sizeof(uint64_t));
    *magic = 0;
    release_block(b);
}

void *realloc(void *ptr, size_t size)
{
    if (ptr == NULL) {
        return malloc(size);
    }
    if (size == 0) {
        free(ptr);
        return NULL;
    }
    block_t *b = get_block(ptr);
    if (b == NULL) {
        return NULL;
    }
    size_t old_size = b->requested_size;
    if (size <= old_size) {
        b->requested_size = size;
        return ptr;
    }
    void *replacement = malloc(size);
    if (replacement == NULL) {
        return NULL;
    }
    (void)memcpy(replacement, ptr, old_size);
    free(ptr);
    return replacement;
}

int posix_memalign(void **memptr, size_t alignment, size_t size)
{
    if (memptr == NULL || alignment < sizeof(void *) || !is_power_of_two(alignment) ||
        alignment % sizeof(void *) != 0) {
        return 22; /* EINVAL */
    }
    void *result = allocate_aligned(alignment, size);
    if (result == NULL) {
        return 12; /* ENOMEM */
    }
    *memptr = result;
    return 0;
}

void *memcpy(void *restrict dest, const void *restrict src, size_t n)
{
    unsigned char *d = dest;
    const unsigned char *s = src;
    for (size_t i = 0; i < n; ++i) d[i] = s[i];
    return dest;
}

void *memmove(void *dest, const void *src, size_t n)
{
    unsigned char *d = dest;
    const unsigned char *s = src;
    if ((uintptr_t)d < (uintptr_t)s) {
        for (size_t i = 0; i < n; ++i) d[i] = s[i];
    } else if ((uintptr_t)d > (uintptr_t)s) {
        while (n != 0) { --n; d[n] = s[n]; }
    }
    return dest;
}

void *memset(void *dest, int c, size_t n)
{
    unsigned char *d = dest;
    for (size_t i = 0; i < n; ++i) d[i] = (unsigned char)c;
    return dest;
}

int memcmp(const void *lhs, const void *rhs, size_t n)
{
    const unsigned char *a = lhs, *b = rhs;
    for (size_t i = 0; i < n; ++i) if (a[i] != b[i]) return (int)a[i] - (int)b[i];
    return 0;
}

void *memchr(const void *s, int c, size_t n)
{
    const unsigned char *p = s;
    for (size_t i = 0; i < n; ++i) if (p[i] == (unsigned char)c) return (void *)(p + i);
    return NULL;
}

size_t strlen(const char *s)
{
    size_t n = 0;
    while (s[n] != '\0') ++n;
    return n;
}

size_t strnlen(const char *s, size_t maxlen)
{
    size_t n = 0;
    while (n < maxlen && s[n] != '\0') ++n;
    return n;
}

int strcmp(const char *lhs, const char *rhs)
{
    while (*lhs != '\0' && (unsigned char)*lhs == (unsigned char)*rhs) { ++lhs; ++rhs; }
    return (int)(unsigned char)*lhs - (int)(unsigned char)*rhs;
}

int strncmp(const char *lhs, const char *rhs, size_t n)
{
    for (size_t i = 0; i < n; ++i) {
        unsigned char a = (unsigned char)lhs[i], b = (unsigned char)rhs[i];
        if (a != b || a == '\0') return (int)a - (int)b;
    }
    return 0;
}

size_t strcspn(const char *s, const char *reject)
{
    size_t length = 0;
    while (s[length] != '\0') {
        for (size_t i = 0; reject[i] != '\0'; ++i) {
            if (s[length] == reject[i]) return length;
        }
        ++length;
    }
    return length;
}

char *strcpy(char *restrict dest, const char *restrict src)
{
    char *result = dest;
    do { *dest++ = *src; } while (*src++ != '\0');
    return result;
}

char *strncpy(char *restrict dest, const char *restrict src, size_t n)
{
    char *result = dest;
    size_t i = 0;
    while (i < n && src[i] != '\0') { dest[i] = src[i]; ++i; }
    while (i < n) dest[i++] = '\0';
    return result;
}

char *strrchr(const char *s, int c)
{
    const char *last = NULL;
    do { if ((unsigned char)*s == (unsigned char)c) last = s; } while (*s++ != '\0');
    return (char *)last;
}

char *strdup(const char *s)
{
    size_t length = strlen(s);
    if (length == SIZE_MAX) return NULL;
    char *copy = malloc(length + 1u);
    if (copy != NULL) (void)memcpy(copy, s, length + 1u);
    return copy;
}

static void swap_elements(unsigned char *a, unsigned char *b, size_t size)
{
    for (size_t i = 0; i < size; ++i) {
        unsigned char tmp = a[i];
        a[i] = b[i];
        b[i] = tmp;
    }
}

static void sift_down(unsigned char *base, size_t root, size_t end, size_t size,
                      int (*compare)(const void *, const void *))
{
    while (root < end / 2u) {
        size_t child = root * 2u + 1u;
        if (child + 1u < end &&
            compare(base + child * size, base + (child + 1u) * size) < 0) {
            ++child;
        }
        if (compare(base + root * size, base + child * size) >= 0) {
            return;
        }
        swap_elements(base + root * size, base + child * size, size);
        root = child;
    }
}

void qsort(void *array, size_t count, size_t size,
           int (*compare)(const void *, const void *))
{
    if (array == NULL || compare == NULL || size == 0 || count < 2 || count > SIZE_MAX / size) {
        return;
    }
    unsigned char *base = array;
    for (size_t start = count / 2u; start != 0;) {
        --start;
        sift_down(base, start, count, size, compare);
    }
    for (size_t end = count; end > 1;) {
        --end;
        swap_elements(base, base + end * size, size);
        sift_down(base, 0, end, size, compare);
    }
}
