#include "slint_frame.h"

#include <assert.h>

static uint8_t surface[SLINT_FRAME_REGION_BYTES];
_Alignas(4) static uint8_t bgra[SLINT_FRAME_WIDTH * SLINT_FRAME_HEIGHT * 4u];
_Alignas(4) static uint8_t unaligned[SLINT_FRAME_WIDTH * SLINT_FRAME_HEIGHT * 4u + 1u];

int main(void)
{
    slint_frame_header_t *header = (slint_frame_header_t *)surface;
    *header = (slint_frame_header_t) {
        .revision = ATOMIC_VAR_INIT(0),
        .width = SLINT_FRAME_WIDTH,
        .height = SLINT_FRAME_HEIGHT,
        .stride_bytes = SLINT_FRAME_STRIDE_BYTES,
        .format = SLINT_FRAME_FORMAT_RGB8,
    };
    uint32_t revision = 0;
    size_t required = sizeof(*header) + SLINT_FRAME_PIXEL_BYTES;

    assert(!slint_frame_header_valid(header, required, &revision));
    atomic_store_explicit(&header->revision, 2, memory_order_release);
    assert(slint_frame_header_valid(header, required, &revision));
    assert(revision == 2);
    assert(slint_frame_revision_unchanged(header, revision));
    assert(!slint_frame_header_valid(header, required - 1, &revision));

    uint8_t *rgb = surface + sizeof(*header);
    rgb[0] = 0x11;
    rgb[1] = 0x22;
    rgb[2] = 0x33;
    rgb[3] = 0x44;
    rgb[4] = 0x55;
    rgb[5] = 0x66;
    assert(slint_frame_copy_to_bgra(header, rgb, sizeof(surface), bgra, sizeof(bgra)));
    assert(bgra[0] == 0x33 && bgra[1] == 0x22 && bgra[2] == 0x11 && bgra[3] == 0xff);
    assert(bgra[4] == 0x66 && bgra[5] == 0x55 && bgra[6] == 0x44 && bgra[7] == 0xff);
    assert(!slint_frame_copy_to_bgra(header, rgb, sizeof(surface), bgra, sizeof(bgra) - 1));

    /* The band path must preserve other rows and support an unaligned target. */
    size_t rgb_row = SLINT_FRAME_STRIDE_BYTES;
    size_t dst_row = SLINT_FRAME_WIDTH * 4u;
    rgb[rgb_row] = 0x12;
    rgb[rgb_row + 1] = 0x34;
    rgb[rgb_row + 2] = 0x56;
    bgra[0] = 0x9a;
    unaligned[1] = 0x9b;
    assert(slint_frame_copy_band_to_bgra(header, rgb, sizeof(surface), bgra, sizeof(bgra), 1, 1));
    assert(slint_frame_copy_band_to_bgra(header, rgb, sizeof(surface), unaligned + 1, sizeof(bgra), 1, 1));
    assert(bgra[0] == 0x9a && unaligned[1] == 0x9b);
    assert(bgra[dst_row] == 0x56 && bgra[dst_row + 1] == 0x34 &&
           bgra[dst_row + 2] == 0x12 && bgra[dst_row + 3] == 0xff);
    for (size_t i = 0; i < dst_row; i++) assert(bgra[dst_row + i] == unaligned[1 + dst_row + i]);

    /* If two non-overlapping updates were published, both must reach scanout. */
    uint32_t copied, y, height;
    size_t row_a = 10u * rgb_row, row_b = 20u * rgb_row;
    rgb[row_a] = 0x71;
    rgb[row_b] = 0x93;
    atomic_store_explicit(&header->revision, 6, memory_order_release);
    atomic_store_explicit(&header->damage_y, 20, memory_order_relaxed);
    atomic_store_explicit(&header->damage_height, 1, memory_order_relaxed);
    assert(slint_frame_copy_update_to_bgra(header, rgb, sizeof(surface), bgra, sizeof(bgra),
                                           2, &copied, &y, &height));
    assert(copied == 6 && y == 0 && height == SLINT_FRAME_HEIGHT);
    assert(bgra[10u * dst_row + 2] == 0x71 && bgra[20u * dst_row + 2] == 0x93);

    /* A consecutive update still copies just the changed band. */
    rgb[row_b] = 0xa4;
    atomic_store_explicit(&header->revision, 8, memory_order_release);
    assert(slint_frame_copy_update_to_bgra(header, rgb, sizeof(surface), bgra, sizeof(bgra),
                                           6, &copied, &y, &height));
    assert(copied == 8 && y == 20 && height == 1);
    assert(bgra[20u * dst_row + 2] == 0xa4);

    /* After an interrupted copy the consumer forces a fresh whole frame. */
    assert(slint_frame_copy_update_to_bgra(header, rgb, sizeof(surface), bgra, sizeof(bgra),
                                           UINT32_MAX, &copied, &y, &height));
    assert(y == 0 && height == SLINT_FRAME_HEIGHT);

    /* Revision zero is invalid during wrap, so the first valid revision
     * after wrap must conservatively include the skipped publication. */
    atomic_store_explicit(&header->revision, 2, memory_order_release);
    assert(slint_frame_copy_update_to_bgra(header, rgb, sizeof(surface), bgra, sizeof(bgra),
                                           UINT32_MAX - 1u, &copied, &y, &height));
    assert(y == 0 && height == SLINT_FRAME_HEIGHT);
    atomic_store_explicit(&header->revision, 3, memory_order_release);
    assert(!slint_frame_copy_update_to_bgra(header, rgb, sizeof(surface), bgra, sizeof(bgra),
                                            2, &copied, &y, &height));

    atomic_store_explicit(&header->revision, 3, memory_order_release);
    assert(!slint_frame_header_valid(header, required, &revision));
    atomic_store_explicit(&header->revision, 4, memory_order_release);
    assert(!slint_frame_revision_unchanged(header, revision));

    header->width++;
    assert(!slint_frame_header_valid(header, required, &revision));
    header->width = SLINT_FRAME_WIDTH;
    header->stride_bytes--;
    assert(!slint_frame_header_valid(header, required, &revision));
    header->stride_bytes = SLINT_FRAME_STRIDE_BYTES;
    header->format++;
    assert(!slint_frame_header_valid(header, required, &revision));
    return 0;
}
