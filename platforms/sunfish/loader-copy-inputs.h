/* Read-only relocation argument snapshot; splash and live stack writes only. */
#ifndef SUNFISH_LOADER_COPY_INPUTS_H
#define SUNFISH_LOADER_COPY_INPUTS_H
#include <stddef.h>
#include <stdint.h>

enum loader_copy_input_error {
    LOADER_INPUT_SOURCE = 1, LOADER_INPUT_DESTINATION = 2,
    LOADER_INPUT_NULL = 4, LOADER_INPUT_EMPTY_PREFIX = 8,
    LOADER_INPUT_PREFIX_ORDER = 16, LOADER_INPUT_PAYLOAD_ORDER = 32,
    LOADER_INPUT_TOO_LARGE = 64, LOADER_INPUT_SOURCE_OVERFLOW = 128,
    LOADER_INPUT_DEST_OVERFLOW = 256, LOADER_INPUT_OVERLAP = 512
};
static inline uint32_t loader_copy_input_errors(uintptr_t source, uintptr_t destination,
                                               size_t prefix_end, size_t payload_start, size_t total)
{
    uint32_t error = 0;
    if (source != 0xb0081000) error |= LOADER_INPUT_SOURCE;
    if (destination != 0xc0000000) error |= LOADER_INPUT_DESTINATION;
    if (!source || !destination) error |= LOADER_INPUT_NULL;
    if (!prefix_end) error |= LOADER_INPUT_EMPTY_PREFIX;
    if (prefix_end > payload_start) error |= LOADER_INPUT_PREFIX_ORDER;
    if (payload_start > total) error |= LOADER_INPUT_PAYLOAD_ORDER;
    if (total > 0x5000000) error |= LOADER_INPUT_TOO_LARGE;
    if (source > UINTPTR_MAX - total) error |= LOADER_INPUT_SOURCE_OVERFLOW;
    if (destination > UINTPTR_MAX - total) error |= LOADER_INPUT_DEST_OVERFLOW;
    if (!(error & (LOADER_INPUT_SOURCE_OVERFLOW | LOADER_INPUT_DEST_OVERFLOW)) &&
        !(source + total <= destination || destination + total <= source)) error |= LOADER_INPUT_OVERLAP;
    return error;
}

/* One value per row, preserving all 64 bits without pointer tables that
 * would contain linked addresses during execution at the source. */
static inline void loader_copy_input_hex(char text[25], char a, char b, char c, uint64_t value)
{
    text[0] = a; text[1] = b; text[2] = c; text[3] = ' ';
    for (unsigned n = 0; n < 16; n++)
        text[4 + n] = "0123456789ABCDEF"[(value >> (60 - n * 4)) & 15];
    text[20] = 0;
}
static inline void loader_copy_input_rows(char rows[9][25], uintptr_t source, uintptr_t destination,
                                         size_t prefix_end, size_t payload_start, size_t total)
{
    const char title[] = "IMAGE COPY INPUTS";
    const char scope[] = "NO COPY OR KERNEL";
    const char restart[] = "RESTART WHEN DONE";
    for (size_t n = 0; n < sizeof(title); n++) rows[0][n] = title[n];
    loader_copy_input_hex(rows[1], 'S', 'R', 'C', source);
    loader_copy_input_hex(rows[2], 'D', 'S', 'T', destination);
    loader_copy_input_hex(rows[3], 'P', 'R', 'E', prefix_end);
    loader_copy_input_hex(rows[4], 'P', 'A', 'Y', payload_start);
    loader_copy_input_hex(rows[5], 'L', 'E', 'N', total);
    loader_copy_input_hex(rows[6], 'E', 'R', 'R', loader_copy_input_errors(source, destination, prefix_end, payload_start, total));
    for (size_t n = 0; n < sizeof(scope); n++) rows[7][n] = scope[n];
    for (size_t n = 0; n < sizeof(restart); n++) rows[8][n] = restart[n];
}

#ifdef SUNFISH_LOADER_COPY_INPUTS
/* Same font, scale and two buffers as the single-line entry marker. Each
 * row owns its 96-pixel strip; the rest of each framebuffer is preserved. */
static void loader_copy_input_display(char rows[9][25])
{
    volatile uint32_t *fb = CHECKPOINT_FB;
    for (unsigned buffer = 0; buffer < 2; buffer++) {
        unsigned base = buffer * 1080 * 2340;
        for (unsigned row = 0; row < 9; row++) {
            unsigned top = 160 + row * 96;
            for (unsigned y = top; y < top + 96; y++)
                for (unsigned x = 0; x < 1080; x++) fb[base + y * 1080 + x] = 0x00122b52;
            for (unsigned column = 0; column < 24 && rows[row][column]; column++) {
                unsigned c = (unsigned char)rows[row][column], glyph;
                if (c >= 'A' && c <= 'Z') glyph = c - 'A';
                else if (c >= '0' && c <= '9') glyph = 26 + c - '0';
                else continue;
                for (unsigned gy = 0; gy < 7; gy++) for (unsigned gx = 0; gx < 5; gx++)
                    if (checkpoint_font[glyph][gy] & (1u << (4 - gx)))
                        for (unsigned dy = 0; dy < 7; dy++) for (unsigned dx = 0; dx < 7; dx++)
                            fb[base + (top + 40 + gy * 7 + dy) * 1080 + 48 + column * 42 + gx * 7 + dx] = 0x0000e5e5;
            }
        }
    }
#if defined(__aarch64__)
    __asm__ volatile("dsb sy" ::: "memory");
#endif
}
_Noreturn void loader_copy_hold_stop(void);
_Noreturn void loader_copy_inputs_hold(uintptr_t source, uintptr_t destination,
                                      size_t prefix_end, size_t payload_start, size_t total);
_Noreturn void loader_copy_inputs_hold(uintptr_t source, uintptr_t destination,
                                      size_t prefix_end, size_t payload_start, size_t total)
{
    char rows[9][25];
    loader_copy_input_rows(rows, source, destination, prefix_end, payload_start, total);
    loader_copy_input_display(rows);
    loader_copy_hold_stop();
}
#endif
#endif
