#ifndef _STDIO_H
#define _STDIO_H
#include <stddef.h>
#define BUFSIZ 1024
#define EOF (-1)
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
typedef struct _rts_file FILE;
extern FILE *stdin;
extern FILE *stdout;
extern FILE *stderr;
int printf(const char *, ...);
int sprintf(char *, const char *, ...);
int snprintf(char *, size_t, const char *, ...);
int vsnprintf(char *, size_t, const char *, __builtin_va_list);
int vfprintf(FILE *, const char *, __builtin_va_list);
int fprintf(FILE *, const char *, ...);
int getc(FILE *);
size_t fread(void *, size_t, size_t, FILE *);
size_t fwrite(const void *, size_t, size_t, FILE *);
int puts(const char *);
int fputs(const char *, FILE *);
int fputc(int, FILE *);
int fflush(FILE *);
FILE *fopen(const char *, const char *);
int fclose(FILE *);
#endif
