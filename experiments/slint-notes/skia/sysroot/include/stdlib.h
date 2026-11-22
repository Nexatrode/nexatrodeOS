#ifndef GUEST_STDLIB_H
#define GUEST_STDLIB_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>
#include <locale.h>
typedef struct { int quot, rem; } div_t;
typedef struct { long quot, rem; } ldiv_t;
typedef struct { long long quot, rem; } lldiv_t;
div_t div(int, int);
ldiv_t ldiv(long, long);
lldiv_t lldiv(long long, long long);
void *malloc(size_t);
void *calloc(size_t, size_t);
void *realloc(void *, size_t);
void free(void *);
void *aligned_alloc(size_t, size_t);
int posix_memalign(void **, size_t, size_t);
void abort(void) __attribute__((noreturn));
void exit(int) __attribute__((noreturn));
int atexit(void (*)(void));
int atoi(const char *);
long atol(const char *);
long long atoll(const char *);
long strtol(const char *, char **, int);
unsigned long strtoul(const char *, char **, int);
long long strtoll(const char *, char **, int);
unsigned long long strtoull(const char *, char **, int);
double strtod(const char *, char **);
float strtof(const char *, char **);
long double strtold(const char *, char **);
double strtod_l(const char *, char **, locale_t);
float strtof_l(const char *, char **, locale_t);
long double strtold_l(const char *, char **, locale_t);
double atof(const char *);
int abs(int);
long labs(long);
void qsort(void *, size_t, size_t, int (*)(const void *, const void *));
void *bsearch(const void *, const void *, size_t, size_t, int (*)(const void *, const void *));
char *getenv(const char *);
int setenv(const char *, const char *, int);
int unsetenv(const char *);
int rand(void);
void srand(unsigned);
#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1
#define RAND_MAX 2147483647

#ifdef __cplusplus
}
#endif
#endif
