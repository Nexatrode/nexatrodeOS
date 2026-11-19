#include <ctype.h>
#include <errno.h>
#include <langinfo.h>
#include <locale.h>
#include <math.h>
#include <nl_types.h>
#include <pthread.h>
#include <semaphore.h>
#include <setjmp.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

extern "C" {

void *malloc(size_t);
void *realloc(void *, size_t);
void free(void *);

char __dso_handle;

static size_t copy_text(char *dst, const char *src, size_t n) {
    size_t len = 0;
    while (src[len] != 0) {
        ++len;
    }
    if (n) {
        size_t copy = len < n - 1 ? len : n - 1;
        for (size_t i = 0; i < copy; ++i) {
            dst[i] = src[i];
        }
        dst[copy] = 0;
    }
    return len;
}

int *__errno_location(void) {
    static int value;
    return &value;
}

void abort(void) {
    __builtin_trap();
}

int atexit(void (*)(void)) { return 0; }

int __cxa_atexit(void (*)(void *), void *, void *) { return 0; }

void __cxa_pure_virtual(void) { abort(); }

int __cxa_guard_acquire(unsigned long long *guard) {
    if (*guard & 1) {
        return 0;
    }
    return 1;
}

void __cxa_guard_release(unsigned long long *guard) { *guard = 1; }

void __cxa_guard_abort(unsigned long long *) {}

extern void (*__init_array_start[])(void);
extern void (*__init_array_end[])(void);

void skia_run_ctors(void) {
    for (void (**ctor)(void) = __init_array_start; ctor < __init_array_end; ++ctor) {
        (*ctor)();
    }
}

unsigned long getauxval(unsigned long) { return 0; }

char *getenv(const char *) { return nullptr; }

int abs(int value) { return value < 0 ? -value : value; }

__attribute__((weak)) void *memcpy(void *dst, const void *src, size_t n) {
    auto *d = static_cast<unsigned char *>(dst);
    auto *s = static_cast<const unsigned char *>(src);
    for (size_t i = 0; i < n; ++i) {
        d[i] = s[i];
    }
    return dst;
}

__attribute__((weak)) void *memmove(void *dst, const void *src, size_t n) {
    auto *d = static_cast<unsigned char *>(dst);
    auto *s = static_cast<const unsigned char *>(src);
    if (d < s) {
        for (size_t i = 0; i < n; ++i) {
            d[i] = s[i];
        }
    } else if (d > s) {
        for (size_t i = n; i > 0; --i) {
            d[i - 1] = s[i - 1];
        }
    }
    return dst;
}

__attribute__((weak)) void *memset(void *dst, int value, size_t n) {
    auto *d = static_cast<unsigned char *>(dst);
    for (size_t i = 0; i < n; ++i) {
        d[i] = static_cast<unsigned char>(value);
    }
    return dst;
}

__attribute__((weak)) int memcmp(const void *a, const void *b, size_t n) {
    auto *x = static_cast<const unsigned char *>(a);
    auto *y = static_cast<const unsigned char *>(b);
    for (size_t i = 0; i < n; ++i) {
        if (x[i] != y[i]) {
            return x[i] < y[i] ? -1 : 1;
        }
    }
    return 0;
}

void *memchr(const void *src, int value, size_t n) {
    auto *s = static_cast<const unsigned char *>(src);
    for (size_t i = 0; i < n; ++i) {
        if (s[i] == static_cast<unsigned char>(value)) {
            return const_cast<unsigned char *>(s + i);
        }
    }
    return nullptr;
}

size_t strlen(const char *s) {
    size_t n = 0;
    while (s[n] != 0) {
        ++n;
    }
    return n;
}

int strcmp(const char *a, const char *b) {
    while (*a && *a == *b) {
        ++a;
        ++b;
    }
    return static_cast<unsigned char>(*a) - static_cast<unsigned char>(*b);
}

int strncmp(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; ++i) {
        if (a[i] != b[i] || a[i] == 0) {
            return static_cast<unsigned char>(a[i]) - static_cast<unsigned char>(b[i]);
        }
    }
    return 0;
}

char *strcpy(char *dst, const char *src) {
    char *out = dst;
    while ((*dst++ = *src++) != 0) {
    }
    return out;
}

char *strncpy(char *dst, const char *src, size_t n) {
    size_t i = 0;
    for (; i < n && src[i] != 0; ++i) {
        dst[i] = src[i];
    }
    for (; i < n; ++i) {
        dst[i] = 0;
    }
    return dst;
}

char *strcat(char *dst, const char *src) {
    strcpy(dst + strlen(dst), src);
    return dst;
}

char *strrchr(const char *s, int value) {
    const char *found = nullptr;
    for (; *s; ++s) {
        if (*s == static_cast<char>(value)) {
            found = s;
        }
    }
    if (value == 0) {
        return const_cast<char *>(s);
    }
    return const_cast<char *>(found);
}

char *strstr(const char *haystack, const char *needle) {
    size_t n = strlen(needle);
    if (n == 0) {
        return const_cast<char *>(haystack);
    }
    for (; *haystack; ++haystack) {
        if (strncmp(haystack, needle, n) == 0) {
            return const_cast<char *>(haystack);
        }
    }
    return nullptr;
}

int isdigit(int value) { return value >= '0' && value <= '9'; }

int tolower(int value) {
    if (value >= 'A' && value <= 'Z') {
        return value + ('a' - 'A');
    }
    return value;
}

long strtol(const char *s, char **end, int base) {
    while (*s == ' ' || *s == '\t') {
        ++s;
    }
    int sign = 1;
    if (*s == '+' || *s == '-') {
        sign = *s == '-' ? -1 : 1;
        ++s;
    }
    if (base == 0) {
        base = 10;
    }
    long value = 0;
    const char *start = s;
    while (*s) {
        int digit;
        if (*s >= '0' && *s <= '9') {
            digit = *s - '0';
        } else if (*s >= 'a' && *s <= 'z') {
            digit = *s - 'a' + 10;
        } else if (*s >= 'A' && *s <= 'Z') {
            digit = *s - 'A' + 10;
        } else {
            break;
        }
        if (digit >= base) {
            break;
        }
        value = value * base + digit;
        ++s;
    }
    if (end) {
        *end = const_cast<char *>(s == start ? s - (sign != 1 || *(s - 1) == '+') : s);
    }
    return sign * value;
}

unsigned long long strtoull(const char *s, char **end, int base) {
    return static_cast<unsigned long long>(strtol(s, end, base));
}

static void qsort_swap(unsigned char *a, unsigned char *b, size_t size) {
    for (size_t i = 0; i < size; ++i) {
        unsigned char tmp = a[i];
        a[i] = b[i];
        b[i] = tmp;
    }
}

static void qsort_rec(unsigned char *base, size_t count, size_t size, int (*cmp)(const void *, const void *)) {
    if (count < 2) {
        return;
    }
    unsigned char *pivot = base + (count / 2) * size;
    qsort_swap(base, pivot, size);
    size_t store = 1;
    for (size_t i = 1; i < count; ++i) {
        if (cmp(base + i * size, base) < 0) {
            qsort_swap(base + i * size, base + store * size, size);
            ++store;
        }
    }
    qsort_swap(base, base + (store - 1) * size, size);
    qsort_rec(base, store - 1, size, cmp);
    qsort_rec(base + store * size, count - store, size, cmp);
}

void qsort(void *base, size_t count, size_t size, int (*cmp)(const void *, const void *)) {
    if (base && size && cmp) {
        qsort_rec(static_cast<unsigned char *>(base), count, size, cmp);
    }
}

int pthread_once(pthread_once_t *once, void (*function)(void)) {
    if (once && *once == 0 && function) {
        *once = 1;
        function();
    }
    return 0;
}

int pthread_mutex_init(pthread_mutex_t *mutex, const pthread_mutexattr_t *) {
    if (mutex) {
        *mutex = 0;
    }
    return 0;
}
int pthread_mutex_destroy(pthread_mutex_t *) { return 0; }
int pthread_mutex_lock(pthread_mutex_t *mutex) {
    if (mutex) {
        *mutex = 1;
    }
    return 0;
}
int pthread_mutex_trylock(pthread_mutex_t *mutex) { return pthread_mutex_lock(mutex); }
int pthread_mutex_unlock(pthread_mutex_t *mutex) {
    if (mutex) {
        *mutex = 0;
    }
    return 0;
}
int pthread_mutexattr_init(pthread_mutexattr_t *attr) {
    if (attr) {
        *attr = 0;
    }
    return 0;
}
int pthread_mutexattr_destroy(pthread_mutexattr_t *) { return 0; }
int pthread_mutexattr_settype(pthread_mutexattr_t *, int) { return 0; }
int pthread_cond_init(pthread_cond_t *cond, const pthread_condattr_t *) {
    if (cond) {
        *cond = 0;
    }
    return 0;
}
int pthread_cond_destroy(pthread_cond_t *) { return 0; }
int pthread_cond_signal(pthread_cond_t *) { return 0; }
int pthread_cond_broadcast(pthread_cond_t *) { return 0; }
int pthread_cond_wait(pthread_cond_t *, pthread_mutex_t *) { return 0; }
int pthread_cond_timedwait(pthread_cond_t *, pthread_mutex_t *, const struct timespec *) { return 0; }
pthread_t pthread_self(void) { return 1; }
int pthread_equal(pthread_t a, pthread_t b) { return a == b; }
int pthread_create(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *) { return EAGAIN; }
int pthread_join(pthread_t, void **) { return 0; }
int pthread_detach(pthread_t) { return 0; }
int pthread_key_create(pthread_key_t *key, void (*)(void *)) {
    if (key) {
        *key = 1;
    }
    return 0;
}
int pthread_key_delete(pthread_key_t) { return 0; }
void *pthread_getspecific(pthread_key_t) { return nullptr; }
int pthread_setspecific(pthread_key_t, const void *) { return 0; }

int sem_init(sem_t *sem, int, unsigned value) {
    if (!sem) {
        return -1;
    }
    sem->value = value;
    return 0;
}
int sem_destroy(sem_t *) { return 0; }
int sem_post(sem_t *sem) {
    if (!sem) {
        return -1;
    }
    sem->value += 1;
    return 0;
}
int sem_wait(sem_t *sem) {
    if (!sem) {
        return -1;
    }
    if (sem->value == 0) {
        return 0;
    }
    sem->value -= 1;
    return 0;
}
int sem_trywait(sem_t *sem) {
    if (!sem || sem->value == 0) {
        errno = EAGAIN;
        return -1;
    }
    sem->value -= 1;
    return 0;
}

static void *map_header(void *user) {
    return user ? static_cast<unsigned char *>(user) - sizeof(size_t) : nullptr;
}

void *mmap(void *, size_t length, int, int, int, long) {
    if (length == 0) {
        return MAP_FAILED;
    }
    void *block = malloc(sizeof(size_t) + length);
    if (!block) {
        return MAP_FAILED;
    }
    *static_cast<size_t *>(block) = length;
    return static_cast<unsigned char *>(block) + sizeof(size_t);
}

int munmap(void *addr, size_t) {
    if (addr && addr != MAP_FAILED) {
        free(map_header(addr));
    }
    return 0;
}

int mprotect(void *, size_t, int) { return 0; }

FILE *fopen(const char *, const char *) { return nullptr; }
size_t fread(void *, size_t, size_t, FILE *) { return 0; }
int fclose(FILE *) { return 0; }
int fseek(FILE *, long, int) { return -1; }
long ftell(FILE *) { return -1; }
int fileno(FILE *) { return -1; }
int fstat(int, struct stat *) { return -1; }
int fflush(FILE *) { return 0; }
int ferror(FILE *) { return 0; }

struct Formatter {
    char *out;
    size_t capacity;
    size_t used;
    int overflow;
};

static void emit_char(Formatter *fmt, char ch) {
    if (fmt->used + 1 < fmt->capacity && fmt->out) {
        fmt->out[fmt->used] = ch;
    } else {
        fmt->overflow = 1;
    }
    fmt->used += 1;
}

static void emit_span(Formatter *fmt, const char *text, size_t length) {
    for (size_t i = 0; i < length; ++i) {
        emit_char(fmt, text[i]);
    }
}

static void emit_unsigned(Formatter *fmt, unsigned long long value, int base, int upper, int width, char pad) {
    char digits[64];
    int count = 0;
    if (value == 0) {
        digits[count++] = '0';
    }
    while (value) {
        unsigned digit = value % static_cast<unsigned>(base);
        digits[count++] = static_cast<char>(digit < 10 ? '0' + digit : (upper ? 'A' : 'a') + digit - 10);
        value /= static_cast<unsigned>(base);
    }
    while (width > count) {
        emit_char(fmt, pad);
        --width;
    }
    while (count > 0) {
        emit_char(fmt, digits[--count]);
    }
}

static void emit_signed(Formatter *fmt, long long value, int width, char pad) {
    unsigned long long magnitude;
    if (value < 0) {
        emit_char(fmt, '-');
        magnitude = static_cast<unsigned long long>(-value);
        if (width > 0) {
            --width;
        }
    } else {
        magnitude = static_cast<unsigned long long>(value);
    }
    emit_unsigned(fmt, magnitude, 10, 0, width, pad);
}

static void emit_float(Formatter *fmt, double value, int precision) {
    if (precision < 0) {
        precision = 6;
    }
    if (precision > 9) {
        precision = 9;
    }
    if (value < 0) {
        emit_char(fmt, '-');
        value = -value;
    }
    double scale = 1.0;
    for (int i = 0; i < precision; ++i) {
        scale *= 10.0;
    }
    unsigned long long scaled = static_cast<unsigned long long>(value * scale + 0.5);
    unsigned long long factor = static_cast<unsigned long long>(scale);
    emit_unsigned(fmt, factor ? scaled / factor : scaled, 10, 0, 0, ' ');
    if (precision > 0) {
        emit_char(fmt, '.');
        unsigned long long fraction = factor ? scaled % factor : 0;
        char digits[16];
        for (int i = precision - 1; i >= 0; --i) {
            digits[i] = static_cast<char>('0' + (fraction % 10));
            fraction /= 10;
        }
        emit_span(fmt, digits, static_cast<size_t>(precision));
    }
}

static int format_text(char *out, size_t capacity, const char *format, va_list args) {
    Formatter fmt{out, capacity, 0, 0};
    while (*format) {
        if (*format != '%') {
            emit_char(&fmt, *format++);
            continue;
        }
        ++format;
        char pad = ' ';
        int width = 0;
        int precision = -1;
        if (*format == '0') {
            pad = '0';
            ++format;
        }
        while (*format >= '0' && *format <= '9') {
            width = width * 10 + (*format++ - '0');
        }
        if (*format == '.') {
            ++format;
            precision = 0;
            while (*format >= '0' && *format <= '9') {
                precision = precision * 10 + (*format++ - '0');
            }
        }
        int length = 0;
        if (*format == 'l') {
            length = 1;
            ++format;
            if (*format == 'l') {
                length = 2;
                ++format;
            }
        } else if (*format == 'z') {
            length = 2;
            ++format;
        }
        char spec = *format++;
        if (spec == 's') {
            const char *text = va_arg(args, const char *);
            if (!text) {
                text = "(null)";
            }
            size_t n = strlen(text);
            if (precision >= 0 && static_cast<size_t>(precision) < n) {
                n = static_cast<size_t>(precision);
            }
            emit_span(&fmt, text, n);
        } else if (spec == 'c') {
            emit_char(&fmt, static_cast<char>(va_arg(args, int)));
        } else if (spec == 'd' || spec == 'i') {
            long long value = length >= 2 ? va_arg(args, long long) : length == 1 ? va_arg(args, long) : va_arg(args, int);
            emit_signed(&fmt, value, width, pad);
        } else if (spec == 'u' || spec == 'x' || spec == 'X' || spec == 'p') {
            unsigned long long value =
                spec == 'p' || length >= 2 ? va_arg(args, unsigned long long)
                : length == 1             ? va_arg(args, unsigned long)
                                           : va_arg(args, unsigned int);
            if (spec == 'p') {
                emit_span(&fmt, "0x", 2);
            }
            emit_unsigned(&fmt, value, spec == 'u' ? 10 : 16, spec == 'X', width, pad);
        } else if (spec == 'f' || spec == 'g') {
            emit_float(&fmt, va_arg(args, double), precision);
        } else if (spec == '%') {
            emit_char(&fmt, '%');
        } else if (spec != 0) {
            emit_char(&fmt, spec);
        } else {
            break;
        }
    }
    if (capacity > 0 && out) {
        size_t end = fmt.used < capacity ? fmt.used : capacity - 1;
        out[end] = 0;
    }
    return static_cast<int>(fmt.used);
}

int vsnprintf(char *out, size_t capacity, const char *format, va_list args) {
    return format_text(out, capacity, format, args);
}

int snprintf(char *out, size_t capacity, const char *format, ...) {
    va_list args;
    va_start(args, format);
    int written = vsnprintf(out, capacity, format, args);
    va_end(args);
    return written;
}

int vfprintf(FILE *, const char *format, va_list args) {
    char buffer[256];
    return vsnprintf(buffer, sizeof buffer, format, args);
}

int fprintf(FILE *file, const char *format, ...) {
    va_list args;
    va_start(args, format);
    int written = vfprintf(file, format, args);
    va_end(args);
    return written;
}

int printf(const char *format, ...) {
    va_list args;
    va_start(args, format);
    int written = vfprintf(stdout, format, args);
    va_end(args);
    return written;
}

int vasprintf(char **out, const char *format, va_list args) {
    va_list copy;
    va_copy(copy, args);
    int length = vsnprintf(nullptr, 0, format, copy);
    va_end(copy);
    if (length < 0) {
        return -1;
    }
    char *buffer = static_cast<char *>(malloc(static_cast<size_t>(length) + 1));
    if (!buffer) {
        return -1;
    }
    vsnprintf(buffer, static_cast<size_t>(length) + 1, format, args);
    *out = buffer;
    return length;
}

void *calloc(size_t count, size_t size) {
    size_t bytes = count * size;
    void *block = malloc(bytes);
    if (block && bytes) {
        memset(block, 0, bytes);
    }
    return block;
}

int posix_memalign(void **out, size_t, size_t size) {
    void *block = malloc(size);
    if (!block) {
        return ENOMEM;
    }
    *out = block;
    return 0;
}

void *aligned_alloc(size_t, size_t size) { return malloc(size); }

int mkdir(const char *, mode_t) { return -1; }
int access(const char *, int) { return -1; }
int close(int) { return 0; }
long sysconf(int name) {
    if (name == _SC_PAGESIZE) {
        return 4096;
    }
    if (name == _SC_NPROCESSORS_ONLN) {
        return 1;
    }
    return -1;
}
int isatty(int) { return 0; }
ssize_t read(int, void *, size_t) { return -1; }
ssize_t write(int, const void *, size_t) { return -1; }
off_t lseek(int, off_t, int) { return -1; }
int fsync(int) { return 0; }
ssize_t pread(int, void *, size_t, off_t) { return -1; }
ssize_t pwrite(int, const void *, size_t, off_t) { return -1; }

int open(const char *, int, ...) { return -1; }
int stat(const char *, struct stat *) { return -1; }
int remove(const char *) { return -1; }
int rename(const char *, const char *) { return -1; }

locale_t newlocale(int, const char *, locale_t) { return reinterpret_cast<locale_t>(1); }
void freelocale(locale_t) {}
locale_t uselocale(locale_t) { return reinterpret_cast<locale_t>(1); }
locale_t duplocale(locale_t loc) { return loc; }
struct lconv *localeconv(void) {
    static struct lconv conv{};
    static char dot[] = ".";
    static char empty[] = "";
    static char group[] = "";
    conv.decimal_point = dot;
    conv.thousands_sep = empty;
    conv.grouping = group;
    conv.int_curr_symbol = empty;
    conv.currency_symbol = empty;
    conv.mon_decimal_point = dot;
    conv.mon_thousands_sep = empty;
    conv.mon_grouping = group;
    conv.positive_sign = empty;
    conv.negative_sign = empty;
    conv.frac_digits = 2;
    conv.int_frac_digits = 2;
    return &conv;
}
char *setlocale(int, const char *) { return const_cast<char *>("C"); }
char *nl_langinfo(int) { return const_cast<char *>("UTF-8"); }
char *nl_langinfo_l(int, locale_t) { return const_cast<char *>("UTF-8"); }
nl_catd catopen(const char *, int) { return nullptr; }
char *catgets(nl_catd, int, int, const char *fallback) { return const_cast<char *>(fallback); }
int catclose(nl_catd) { return 0; }

int toupper_l(int value, locale_t) {
    if (value >= 'a' && value <= 'z') {
        return value - ('a' - 'A');
    }
    return value;
}
int tolower_l(int value, locale_t) { return tolower(value); }
int strcoll_l(const char *a, const char *b, locale_t) { return strcmp(a, b); }
size_t strxfrm_l(char *dst, const char *src, size_t n, locale_t) { return copy_text(dst, src, n); }
char *strerror_r(int err, char *buf, size_t n) {
    snprintf(buf, n, "errno %d", err);
    return buf;
}
size_t strftime_l(char *dst, size_t n, const char *, const struct tm *, locale_t) {
    if (n) {
        dst[0] = 0;
    }
    return 0;
}

static double parse_double(const char *s, char **end) {
    while (*s == ' ' || *s == '\t') {
        ++s;
    }
    int sign = 1;
    if (*s == '+' || *s == '-') {
        sign = *s == '-' ? -1 : 1;
        ++s;
    }
    double value = 0;
    const char *start = s;
    while (*s >= '0' && *s <= '9') {
        value = value * 10 + (*s++ - '0');
    }
    if (*s == '.') {
        ++s;
        double place = 0.1;
        while (*s >= '0' && *s <= '9') {
            value += (*s++ - '0') * place;
            place *= 0.1;
        }
    }
    if (end) {
        *end = const_cast<char *>(s == start ? s : s);
    }
    return sign * value;
}

float strtof(const char *s, char **end) { return static_cast<float>(parse_double(s, end)); }
double strtod(const char *s, char **end) { return parse_double(s, end); }
long double strtold(const char *s, char **end) { return parse_double(s, end); }
float strtof_l(const char *s, char **end, locale_t) { return strtof(s, end); }
double strtod_l(const char *s, char **end, locale_t) { return strtod(s, end); }
long double strtold_l(const char *s, char **end, locale_t) { return strtold(s, end); }

}  // extern "C"
