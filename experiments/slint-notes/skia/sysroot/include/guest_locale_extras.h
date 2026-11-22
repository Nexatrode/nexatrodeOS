#ifndef GUEST_LOCALE_EXTRAS_H
#define GUEST_LOCALE_EXTRAS_H
#include <locale.h>
#include <stddef.h>
#include <stdio.h>
#include <time.h>
#include <wchar.h>
#ifdef __cplusplus
extern "C" {
#endif
#ifndef MB_CUR_MAX
#define MB_CUR_MAX 1
#endif
int toupper_l(int, locale_t);
int tolower_l(int, locale_t);
int strcoll_l(const char *, const char *, locale_t);
size_t strxfrm_l(char *, const char *, size_t, locale_t);
char *strerror_r(int, char *, size_t);
size_t strftime_l(char *, size_t, const char *, const struct tm *, locale_t);
long wcstol(const wchar_t *, wchar_t **, int);
unsigned long wcstoul(const wchar_t *, wchar_t **, int);
long long wcstoll(const wchar_t *, wchar_t **, int);
unsigned long long wcstoull(const wchar_t *, wchar_t **, int);
int ungetc(int, FILE *);
wint_t getwc(FILE *);
wint_t fgetwc(FILE *);
wint_t fputwc(wchar_t, FILE *);
wint_t putwc(wchar_t, FILE *);
wint_t ungetwc(wint_t, FILE *);
wchar_t *wcsncat(wchar_t *, const wchar_t *, size_t);
int wcscoll(const wchar_t *, const wchar_t *);
size_t wcsxfrm(wchar_t *, const wchar_t *, size_t);
size_t wcscspn(const wchar_t *, const wchar_t *);
size_t wcsspn(const wchar_t *, const wchar_t *);
wchar_t *wcstok(wchar_t *, const wchar_t *, wchar_t **);
int wmemcmp(const wchar_t *, const wchar_t *, size_t);
wchar_t *wmemcpy(wchar_t *, const wchar_t *, size_t);
wchar_t *wmemmove(wchar_t *, const wchar_t *, size_t);
wchar_t *wmemset(wchar_t *, wchar_t, size_t);
size_t wcsftime(wchar_t *, size_t, const wchar_t *, const struct tm *);
int mbsinit(const mbstate_t *);
float wcstof(const wchar_t *, wchar_t **);
double wcstod(const wchar_t *, wchar_t **);
long double wcstold(const wchar_t *, wchar_t **);
int iswctype(wint_t, unsigned long);
int iswctype_l(wint_t, unsigned long, locale_t);
int iswspace_l(wint_t, locale_t);
int iswprint_l(wint_t, locale_t);
int iswcntrl_l(wint_t, locale_t);
int iswupper_l(wint_t, locale_t);
int iswlower_l(wint_t, locale_t);
int iswalpha_l(wint_t, locale_t);
int iswblank_l(wint_t, locale_t);
int iswdigit_l(wint_t, locale_t);
int iswpunct_l(wint_t, locale_t);
int iswxdigit_l(wint_t, locale_t);
wint_t towupper_l(wint_t, locale_t);
wint_t towlower_l(wint_t, locale_t);
int wcscoll_l(const wchar_t *, const wchar_t *, locale_t);
size_t wcsxfrm_l(wchar_t *, const wchar_t *, size_t, locale_t);
wint_t wctob(wint_t);
size_t wcsnrtombs(char *, const wchar_t **, size_t, size_t, mbstate_t *);
size_t wcrtomb(char *, wchar_t, mbstate_t *);
size_t mbsnrtowcs(wchar_t *, const char **, size_t, size_t, mbstate_t *);
size_t mbrlen(const char *, size_t, mbstate_t *);
int mbtowc(wchar_t *, const char *, size_t);
size_t mbsrtowcs(wchar_t *, const char **, size_t, mbstate_t *);
#ifdef __cplusplus
}
#endif
#endif
