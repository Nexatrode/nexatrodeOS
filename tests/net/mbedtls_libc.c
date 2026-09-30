/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Freestanding libc bits required by Mbed TLS X.509 on the net fragment.
 */

#include <stddef.h>

char *strchr(const char *s, int c)
{
    if (s == NULL) {
        return NULL;
    }
    for (; *s != '\0'; s++) {
        if ((unsigned char)*s == (unsigned char)c) {
            return (char *)s;
        }
    }
    if (c == '\0') {
        return (char *)s;
    }
    return NULL;
}
