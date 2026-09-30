/* Freestanding stdio stubs for Mbed TLS on the net fragment. */
#pragma once

#include <stdarg.h>
#include <stddef.h>

int printf(const char *format, ...);
int snprintf(char *str, size_t size, const char *format, ...);
int vsnprintf(char *str, size_t size, const char *format, va_list ap);
int fprintf(void *stream, const char *format, ...);
