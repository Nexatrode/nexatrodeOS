/* Bounded RGB frame contract shared by the Slint Notes PD and GPU client. */
#ifndef PERSONAL_OS_SLINT_FRAME_H
#define PERSONAL_OS_SLINT_FRAME_H

#include <stdatomic.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define SLINT_FRAME_WIDTH 300u
#define SLINT_FRAME_HEIGHT 400u
#define SLINT_FRAME_BYTES_PER_PIXEL 3u
#define SLINT_FRAME_STRIDE_BYTES (SLINT_FRAME_WIDTH * SLINT_FRAME_BYTES_PER_PIXEL)
#define SLINT_FRAME_PIXEL_BYTES ((size_t)SLINT_FRAME_STRIDE_BYTES * SLINT_FRAME_HEIGHT)
#define SLINT_FRAME_REGION_BYTES 0x80000u
#define SLINT_FRAME_FORMAT_RGB8 1u

typedef struct {
    _Atomic uint32_t revision;
    uint32_t width;
    uint32_t height;
    uint32_t stride_bytes;
    uint32_t format;
} slint_frame_header_t;

_Static_assert(sizeof(slint_frame_header_t) == 20, "Slint frame header wire layout changed");
_Static_assert(sizeof(slint_frame_header_t) + SLINT_FRAME_PIXEL_BYTES <= SLINT_FRAME_REGION_BYTES,
               "Slint frame exceeds its mapped memory region");

static inline bool slint_frame_header_valid(const slint_frame_header_t *header,
                                            size_t mapped_bytes,
                                            uint32_t *revision)
{
    if (header == NULL || revision == NULL) {
        return false;
    }
    uint32_t before = atomic_load_explicit(&header->revision, memory_order_acquire);
    if (before == 0 || (before & 1u) != 0 ||
        header->width != SLINT_FRAME_WIDTH ||
        header->height != SLINT_FRAME_HEIGHT ||
        header->stride_bytes != SLINT_FRAME_STRIDE_BYTES ||
        header->format != SLINT_FRAME_FORMAT_RGB8 ||
        sizeof(*header) + SLINT_FRAME_PIXEL_BYTES > mapped_bytes) {
        return false;
    }
    *revision = before;
    return true;
}

static inline bool slint_frame_revision_unchanged(const slint_frame_header_t *header,
                                                  uint32_t revision)
{
    atomic_thread_fence(memory_order_acquire);
    return atomic_load_explicit(&header->revision, memory_order_acquire) == revision;
}

static inline bool slint_frame_copy_to_bgra(const slint_frame_header_t *header,
                                            const uint8_t *rgb_pixels,
                                            size_t mapped_bytes,
                                            uint8_t *bgra_pixels,
                                            size_t destination_bytes)
{
    uint32_t revision;
    const size_t bgra_bytes = (size_t)SLINT_FRAME_WIDTH * SLINT_FRAME_HEIGHT * 4u;
    if (rgb_pixels == NULL || bgra_pixels == NULL || destination_bytes < bgra_bytes ||
        !slint_frame_header_valid(header, mapped_bytes, &revision)) {
        return false;
    }

    for (size_t source = 0, destination = 0; source < SLINT_FRAME_PIXEL_BYTES;
         source += 3, destination += 4) {
        bgra_pixels[destination] = rgb_pixels[source + 2];
        bgra_pixels[destination + 1] = rgb_pixels[source + 1];
        bgra_pixels[destination + 2] = rgb_pixels[source];
        bgra_pixels[destination + 3] = 0xff;
    }
    return slint_frame_revision_unchanged(header, revision);
}

#endif
