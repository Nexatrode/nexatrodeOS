#ifndef _STDLIB_H
#define _STDLIB_H
#include <stddef.h>
#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1
_Noreturn void abort(void);
_Noreturn void exit(int);
char *getenv(const char *);
long strtol(const char *, char **, int);
unsigned long strtoul(const char *, char **, int);
unsigned long long strtoull(const char *, char **, int);
double atof(const char *);
double strtod(const char *, char **);
int abs(int);
void *malloc(size_t);
void *calloc(size_t, size_t);
void *realloc(void *, size_t);
void free(void *);
int posix_memalign(void **, size_t, size_t);
void qsort(void *, size_t, size_t, int (*)(const void *, const void *));
#endif
