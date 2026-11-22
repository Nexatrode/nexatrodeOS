#include "slint_frame.h"

#include <assert.h>

static uint8_t surface[SLINT_FRAME_REGION_BYTES];
static uint8_t bgra[SLINT_FRAME_WIDTH * SLINT_FRAME_HEIGHT * 4u];

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
