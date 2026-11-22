#include <ctype.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

FILE *stdin;
FILE *stdout;
FILE *stderr;

unsigned long strtoul(const char *s, char **end, int base) {
    return (unsigned long)strtol(s, end, base);
}

long long strtoll(const char *s, char **end, int base) {
    return (long long)strtol(s, end, base);
}

int sscanf(const char *s, const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    int assigned = 0;
    while (*fmt) {
        if (*fmt != '%') {
            if (*fmt == ' ') {
                while (*s == ' ' || *s == '\t') {
                    ++s;
                }
            } else if (*s == *fmt) {
                ++s;
            } else {
                break;
            }
            ++fmt;
            continue;
        }
        ++fmt;
        if (*fmt == 'l') {
            ++fmt;
        }
        if (*fmt == 'd' || *fmt == 'i' || *fmt == 'u' || *fmt == 'x') {
            char *next = 0;
            long value = strtol(s, &next, *fmt == 'x' ? 16 : 10);
            if (next == s) {
                break;
            }
            *va_arg(args, int *) = (int)value;
            s = next;
            ++assigned;
        } else if (*fmt == 'f') {
            char *next = 0;
            float value = strtof(s, &next);
            if (next == s) {
                break;
            }
            *va_arg(args, float *) = value;
            s = next;
            ++assigned;
        } else if (*fmt == 's') {
            char *out = va_arg(args, char *);
            while (*s == ' ') {
                ++s;
            }
            if (*s == 0) {
                break;
            }
            while (*s && *s != ' ') {
                *out++ = *s++;
            }
            *out = 0;
            ++assigned;
        } else {
            break;
        }
        ++fmt;
    }
    va_end(args);
    return assigned;
}

int toupper(int value) {
    if (value >= 'a' && value <= 'z') {
        return value - ('a' - 'A');
    }
    return value;
}

size_t fwrite(const void *ptr, size_t size, size_t count, FILE *file) {
    (void)ptr;
    (void)file;
    return size ? count : 0;
}

int getc(FILE *file) {
    (void)file;
    return EOF;
}

int ungetc(int value, FILE *file) {
    (void)value;
    (void)file;
    return EOF;
}

wint_t getwc(FILE *file) {
    (void)file;
    return WEOF;
}

wint_t fgetwc(FILE *file) { return getwc(file); }

wint_t fputwc(wchar_t value, FILE *file) {
    (void)value;
    (void)file;
    return WEOF;
}

wint_t putwc(wchar_t value, FILE *file) { return fputwc(value, file); }

wint_t ungetwc(wint_t value, FILE *file) {
    (void)value;
    (void)file;
    return WEOF;
}

int swprintf(wchar_t *dst, size_t n, const wchar_t *fmt, ...) {
    (void)fmt;
    if (n) {
        dst[0] = 0;
    }
    return 0;
}

size_t wcslen(const wchar_t *s) {
    size_t n = 0;
    while (s[n]) {
        ++n;
    }
    return n;
}

wchar_t *wmemchr(const wchar_t *s, wchar_t value, size_t n) {
    for (size_t i = 0; i < n; ++i) {
        if (s[i] == value) {
            return (wchar_t *)s + i;
        }
    }
    return 0;
}

wint_t btowc(int value) { return (wint_t)value; }
wint_t wctob(wint_t value) { return (int)value; }
size_t mbrlen(const char *s, size_t n, mbstate_t *ps) { return mbrtowc(0, s, n, ps); }

size_t mbrtowc(wchar_t *out, const char *s, size_t n, mbstate_t *ps) {
    (void)ps;
    if (!s || n == 0) {
        return 0;
    }
    if (out) {
        *out = (unsigned char)*s;
    }
    return *s ? 1 : 0;
}

size_t wcrtomb(char *out, wchar_t value, mbstate_t *ps) {
    (void)ps;
    if (out) {
        out[0] = (char)value;
    }
    return 1;
}

int mbtowc(wchar_t *out, const char *s, size_t n) { return (int)mbrtowc(out, s, n, 0); }

size_t mbsrtowcs(wchar_t *dst, const char **src, size_t len, mbstate_t *ps) {
    (void)ps;
    const char *s = *src;
    size_t n = 0;
    while (n < len && s[n]) {
        if (dst) {
            dst[n] = (unsigned char)s[n];
        }
        ++n;
    }
    if (n < len && dst) {
        dst[n] = 0;
        *src = 0;
    }
    return n;
}

size_t mbsnrtowcs(wchar_t *dst, const char **src, size_t nms, size_t len, mbstate_t *ps) {
    (void)nms;
    return mbsrtowcs(dst, src, len, ps);
}

size_t wcsnrtombs(char *dst, const wchar_t **src, size_t nwc, size_t len, mbstate_t *ps) {
    (void)ps;
    const wchar_t *s = *src;
    size_t n = 0;
    size_t limit = nwc < len ? nwc : len;
    while (n < limit && s[n]) {
        if (dst) {
            dst[n] = (char)s[n];
        }
        ++n;
    }
    return n;
}

int wcscoll_l(const wchar_t *a, const wchar_t *b, locale_t loc) {
    (void)loc;
    while (*a && *a == *b) {
        ++a;
        ++b;
    }
    return *a < *b ? -1 : *a > *b;
}

size_t wcsxfrm_l(wchar_t *dst, const wchar_t *src, size_t n, locale_t loc) {
    (void)loc;
    size_t len = wcslen(src);
    if (n) {
        size_t copy = len < n ? len : n - 1;
        for (size_t i = 0; i < copy; ++i) {
            dst[i] = src[i];
        }
        dst[copy] = 0;
    }
    return len;
}

static int wide_class(wint_t value, int kind) {
    int c = (int)value;
    if (kind == 0) {
        return c == ' ' || c == '\t' || c == '\n';
    }
    if (kind == 1) {
        return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z');
    }
    if (kind == 2) {
        return c >= '0' && c <= '9';
    }
    if (kind == 3) {
        return c > 32 && c < 127;
    }
    if (kind == 4) {
        return c >= 'A' && c <= 'Z';
    }
    if (kind == 5) {
        return c >= 'a' && c <= 'z';
    }
    if (kind == 6) {
        return c == ' ' || c == '\t';
    }
    if (kind == 7) {
        return c < 32 || c == 127;
    }
    if (kind == 8) {
        return (c > 32 && c < 127) && !((c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9'));
    }
    return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
}

int iswspace_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 0); }
int iswalpha_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 1); }
int iswdigit_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 2); }
int iswprint_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 3); }
int iswupper_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 4); }
int iswlower_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 5); }
int iswblank_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 6); }
int iswcntrl_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 7); }
int iswpunct_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 8); }
int iswxdigit_l(wint_t c, locale_t loc) { (void)loc; return wide_class(c, 9); }
wint_t towupper_l(wint_t c, locale_t loc) { (void)loc; return (wint_t)toupper((int)c); }
wint_t towlower_l(wint_t c, locale_t loc) { (void)loc; return (wint_t)tolower((int)c); }

float wcstof(const wchar_t *s, wchar_t **end) {
    char buf[64];
    size_t n = 0;
    while (s[n] && n + 1 < sizeof buf) {
        buf[n] = (char)s[n];
        ++n;
    }
    buf[n] = 0;
    char *narrow_end = 0;
    float value = strtof(buf, &narrow_end);
    if (end) {
        *end = (wchar_t *)s + (narrow_end - buf);
    }
    return value;
}

double wcstod(const wchar_t *s, wchar_t **end) { return wcstof(s, end); }
long double wcstold(const wchar_t *s, wchar_t **end) { return wcstof(s, end); }

static unsigned long long wcstoull_impl(const wchar_t *s, wchar_t **end, int base) {
    char buf[64];
    size_t n = 0;
    while (s[n] && n + 1 < sizeof buf) {
        buf[n] = (char)s[n];
        ++n;
    }
    buf[n] = 0;
    char *narrow_end = 0;
    unsigned long long value = strtoull(buf, &narrow_end, base);
    if (end) {
        *end = (wchar_t *)s + (narrow_end - buf);
    }
    return value;
}

long wcstol(const wchar_t *s, wchar_t **end, int base) { return (long)wcstoull_impl(s, end, base); }
unsigned long wcstoul(const wchar_t *s, wchar_t **end, int base) { return (unsigned long)wcstoull_impl(s, end, base); }
long long wcstoll(const wchar_t *s, wchar_t **end, int base) { return (long long)wcstoull_impl(s, end, base); }
unsigned long long wcstoull(const wchar_t *s, wchar_t **end, int base) { return wcstoull_impl(s, end, base); }

long double __extenddftf2(double value);
long double __extendsftf2(float value) { return __extenddftf2((double)value); }

long double __extenddftf2(double value) {
    union {
        double in;
        unsigned long long bits;
    } source;
    union {
        unsigned __int128 bits;
        long double out;
    } dest;
    source.in = value;
    unsigned long long sign = source.bits >> 63;
    unsigned long long exp = (source.bits >> 52) & 0x7ff;
    unsigned long long sig = source.bits & 0xfffffffffffffULL;
    unsigned __int128 result;
    if (exp == 0) {
        result = (unsigned __int128)sign << 127;
    } else if (exp == 0x7ff) {
        result = ((unsigned __int128)sign << 127) | ((unsigned __int128)0x7fff << 112) | ((unsigned __int128)sig << 60);
    } else {
        unsigned long long new_exp = exp - 1023 + 16383;
        result = ((unsigned __int128)sign << 127) | ((unsigned __int128)new_exp << 112) | ((unsigned __int128)sig << 60);
    }
    dest.bits = result;
    return dest.out;
}
