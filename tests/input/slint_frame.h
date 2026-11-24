/* Bounded RGB frame contract shared by the Slint Notes PD and GPU client. */
#ifndef PERSONAL_OS_SLINT_FRAME_H
#define PERSONAL_OS_SLINT_FRAME_H

#include <stdatomic.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifndef SLINT_FRAME_WIDTH
#define SLINT_FRAME_WIDTH 1080u
#endif
#ifndef SLINT_FRAME_HEIGHT
#define SLINT_FRAME_HEIGHT 2340u
#endif
#define SLINT_FRAME_BYTES_PER_PIXEL 3u
#define SLINT_FRAME_STRIDE_BYTES (SLINT_FRAME_WIDTH * SLINT_FRAME_BYTES_PER_PIXEL)
#define SLINT_FRAME_PIXEL_BYTES ((size_t)SLINT_FRAME_STRIDE_BYTES * SLINT_FRAME_HEIGHT)
#ifndef SLINT_FRAME_REGION_BYTES
#define SLINT_FRAME_REGION_BYTES 0x800000u
#endif
#define SLINT_FRAME_FORMAT_RGB8 1u

typedef struct {
    _Atomic uint32_t revision;
    uint32_t width;
    uint32_t height;
    uint32_t stride_bytes;
    uint32_t format;
    _Atomic uint32_t damage_y;
    _Atomic uint32_t damage_height;
} slint_frame_header_t;

_Static_assert(sizeof(slint_frame_header_t) == 28, "Slint frame header wire layout changed");
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

static inline bool slint_frame_copy_band_to_bgra(const slint_frame_header_t *header,
                                                 const uint8_t *rgb_pixels,
                                                 size_t mapped_bytes,
                                                 uint8_t *bgra_pixels,
                                                 size_t destination_bytes,
                                                 uint32_t y,
                                                 uint32_t height)
{
    uint32_t revision;
    const size_t bgra_bytes = (size_t)SLINT_FRAME_WIDTH * SLINT_FRAME_HEIGHT * 4u;
    if (rgb_pixels == NULL || bgra_pixels == NULL || destination_bytes < bgra_bytes ||
        height == 0 || y >= SLINT_FRAME_HEIGHT || height > SLINT_FRAME_HEIGHT - y ||
        !slint_frame_header_valid(header, mapped_bytes, &revision)) {
        return false;
    }

    const size_t rgb_stride = SLINT_FRAME_STRIDE_BYTES;
    const size_t bgra_stride = (size_t)SLINT_FRAME_WIDTH * 4u;
    const uint8_t *src = rgb_pixels + (size_t)y * rgb_stride;
    uint8_t *dst = bgra_pixels + (size_t)y * bgra_stride;
    for (uint32_t row = 0; row < height; row++) {
        for (uint32_t column = 0; column < SLINT_FRAME_WIDTH; column++) {
            size_t source = (size_t)column * 3u;
            size_t destination = (size_t)column * 4u;
            dst[destination] = src[source + 2];
            dst[destination + 1] = src[source + 1];
            dst[destination + 2] = src[source];
            dst[destination + 3] = 0xff;
        }
        src += rgb_stride;
        dst += bgra_stride;
    }
    return slint_frame_revision_unchanged(header, revision);
}

static inline bool slint_frame_copy_update_to_bgra(const slint_frame_header_t *header,
                                                   const uint8_t *rgb_pixels,
                                                   size_t mapped_bytes,
                                                   uint8_t *bgra_pixels,
                                                   size_t destination_bytes,
                                                   uint32_t previous_revision,
                                                   uint32_t *copied_revision,
                                                   uint32_t *damage_y,
                                                   uint32_t *damage_height)
{
    uint32_t revision;
    if (copied_revision == NULL || damage_y == NULL || damage_height == NULL ||
        !slint_frame_header_valid(header, mapped_bytes, &revision)) {
        return false;
    }
    uint32_t y = atomic_load_explicit(&header->damage_y, memory_order_relaxed);
    uint32_t height = atomic_load_explicit(&header->damage_height, memory_order_relaxed);
    /* The header records only the newest band's damage. If the consumer
     * missed any revisions, the shared RGB image is still complete but the
     * damage metadata is not. Copy it all before resuming partial updates. */
    if (previous_revision == UINT32_MAX || revision - previous_revision != 2u ||
        height == 0 || y >= SLINT_FRAME_HEIGHT || height > SLINT_FRAME_HEIGHT - y) {
        y = 0;
        height = SLINT_FRAME_HEIGHT;
    }
    if (!slint_frame_revision_unchanged(header, revision) ||
        !slint_frame_copy_band_to_bgra(header, rgb_pixels, mapped_bytes, bgra_pixels,
                                       destination_bytes, y, height) ||
        !slint_frame_revision_unchanged(header, revision)) {
        return false;
    }
    *copied_revision = revision;
    *damage_y = y;
    *damage_height = height;
    return true;
}

#endif
