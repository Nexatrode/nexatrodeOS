/*
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Small, bounded C runtime used by the first single-threaded GHC/seL4 image.
 * This is not a general POSIX libc. File, process, and environment services
 * need explicit Microkit capability backends and are intentionally absent.
 */
#pragma once

#include <stddef.h>
#include <sys/types.h>
#include <utime.h>

void *malloc(size_t size);
void *calloc(size_t count, size_t size);
void *realloc(void *ptr, size_t size);
void free(void *ptr);
int posix_memalign(void **memptr, size_t alignment, size_t size);

/* Filesystem calls fail until a filesystem service is connected. */
int link(const char *old_path, const char *new_path);
mode_t umask(mode_t mask);
int utime(const char *path, const struct utimbuf *times);

void *memcpy(void *restrict dest, const void *restrict src, size_t n);
void *memmove(void *dest, const void *src, size_t n);
void *memset(void *dest, int c, size_t n);
int memcmp(const void *lhs, const void *rhs, size_t n);
void *memchr(const void *s, int c, size_t n);

size_t strlen(const char *s);
size_t strnlen(const char *s, size_t maxlen);
int strcmp(const char *lhs, const char *rhs);
int strncmp(const char *lhs, const char *rhs, size_t n);
char *strcpy(char *restrict dest, const char *restrict src);
char *strncpy(char *restrict dest, const char *restrict src, size_t n);
char *strrchr(const char *s, int c);
char *strdup(const char *s);
void qsort(void *base, size_t count, size_t size,
           int (*compare)(const void *, const void *));
