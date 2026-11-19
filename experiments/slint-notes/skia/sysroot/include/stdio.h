#ifndef GUEST_STDIO_H
#define GUEST_STDIO_H
#include <stddef.h>
#include <stdarg.h>
struct __guest_FILE;
typedef struct __guest_FILE FILE;
#ifdef __cplusplus
extern "C" {
#endif
extern FILE *stdin;
extern FILE *stdout;
extern FILE *stderr;
int printf(const char *, ...);
int fprintf(FILE *, const char *, ...);
int sprintf(char *, const char *, ...);
int snprintf(char *, size_t, const char *, ...);
int vsnprintf(char *, size_t, const char *, va_list);
int vfprintf(FILE *, const char *, va_list);
int vprintf(const char *, va_list);
int vasprintf(char **, const char *, va_list);
int puts(const char *);
int fputs(const char *, FILE *);
int fputc(int, FILE *);
int putchar(int);
int getc(FILE *);
int sscanf(const char *, const char *, ...);
FILE *fopen(const char *, const char *);
FILE *fdopen(int, const char *);
int fileno(FILE *);
int fclose(FILE *);
size_t fread(void *, size_t, size_t, FILE *);
size_t fwrite(const void *, size_t, size_t, FILE *);
int fseek(FILE *, long, int);
long ftell(FILE *);
int fflush(FILE *);
int feof(FILE *);
int ferror(FILE *);
void clearerr(FILE *);
void rewind(FILE *);
int remove(const char *);
int rename(const char *, const char *);
void perror(const char *);
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define EOF (-1)
#define BUFSIZ 1024
#ifdef __cplusplus
}
#endif
#endif
