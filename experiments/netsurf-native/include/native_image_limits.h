/* SPDX-License-Identifier: GPL-2.0-only */
#pragma once
#include <stdbool.h>
#include <stdint.h>
#define NATIVE_IMAGE_MAX_SIDE 4096u
#define NATIVE_IMAGE_MAX_PIXELS (4u * 1024u * 1024u)
static inline bool native_image_size_ok(uint32_t w, uint32_t h) {
    return w && h && w <= NATIVE_IMAGE_MAX_SIDE && h <= NATIVE_IMAGE_MAX_SIDE
        && w <= NATIVE_IMAGE_MAX_PIXELS / h;
}
