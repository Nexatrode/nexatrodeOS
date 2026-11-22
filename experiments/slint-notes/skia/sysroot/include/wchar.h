#ifndef GUEST_WCHAR_H
#define GUEST_WCHAR_H
#ifdef __cplusplus
extern "C" {
#endif
#include <stddef.h>
#include <stdarg.h>
#include <bits/types/mbstate_t.h>
#ifndef WEOF
#define WEOF (0xffffffffu)
#endif
typedef unsigned int wint_t;
wint_t btowc(int);
size_t mbrtowc(wchar_t *, const char *, size_t, mbstate_t *);
size_t wcsrtombs(char *, const wchar_t **, size_t, mbstate_t *);
size_t wcslen(const wchar_t *);
wchar_t *wcscpy(wchar_t *, const wchar_t *);
wchar_t *wcsncpy(wchar_t *, const wchar_t *, size_t);
wchar_t *wcscat(wchar_t *, const wchar_t *);
int wcscmp(const wchar_t *, const wchar_t *);
int wcsncmp(const wchar_t *, const wchar_t *, size_t);
wchar_t *wcschr(const wchar_t *, wchar_t);
wchar_t *wcsrchr(const wchar_t *, wchar_t);
wchar_t *wcspbrk(const wchar_t *, const wchar_t *);
wchar_t *wcsstr(const wchar_t *, const wchar_t *);
wchar_t *wmemchr(const wchar_t *, wchar_t, size_t);
int swprintf(wchar_t *, size_t, const wchar_t *, ...);
#ifdef __cplusplus
}
#endif
#endif
