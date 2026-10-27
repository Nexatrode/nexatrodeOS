/* Visible loader-only boot markers. Writes only the two existing splash buffers. */
#ifndef SUNFISH_LOADER_BOOT_CHECKPOINTS_H
#define SUNFISH_LOADER_BOOT_CHECKPOINTS_H
#include <stdint.h>
#ifndef CHECKPOINT_FB
#define CHECKPOINT_FB ((volatile uint32_t *)(uintptr_t)0x9c000000)
#endif
void loader_checkpoint(const char *text);
void loader_checkpoint_hex(const char *title, uint64_t value);
static const uint8_t checkpoint_font[36][7] = {
    {14, 17, 17, 31, 17, 17, 17},
    {30, 17, 17, 30, 17, 17, 30},
    {14, 17, 16, 16, 16, 17, 14},
    {30, 17, 17, 17, 17, 17, 30},
    {31, 16, 16, 30, 16, 16, 31},
    {31, 16, 16, 30, 16, 16, 16},
    {14, 17, 16, 23, 17, 17, 15},
    {17, 17, 17, 31, 17, 17, 17},
    {14, 4, 4, 4, 4, 4, 14},
    {7, 2, 2, 2, 18, 18, 12},
    {17, 18, 20, 24, 20, 18, 17},
    {16, 16, 16, 16, 16, 16, 31},
    {17, 27, 21, 21, 17, 17, 17},
    {17, 25, 21, 19, 17, 17, 17},
    {14, 17, 17, 17, 17, 17, 14},
    {30, 17, 17, 30, 16, 16, 16},
    {14, 17, 17, 17, 21, 18, 13},
    {30, 17, 17, 30, 20, 18, 17},
    {15, 16, 16, 14, 1, 1, 30},
    {31, 4, 4, 4, 4, 4, 4},
    {17, 17, 17, 17, 17, 17, 14},
    {17, 17, 17, 17, 17, 10, 4},
    {17, 17, 17, 21, 21, 21, 10},
    {17, 17, 10, 4, 10, 17, 17},
    {17, 17, 10, 4, 4, 4, 4},
    {31, 1, 2, 4, 8, 16, 31},
    {14, 17, 19, 21, 25, 17, 14},
    {4, 12, 4, 4, 4, 4, 14},
    {14, 17, 1, 2, 4, 8, 31},
    {30, 1, 1, 14, 1, 1, 30},
    {2, 6, 10, 18, 31, 2, 2},
    {31, 16, 16, 30, 1, 1, 30},
    {14, 16, 16, 30, 17, 17, 14},
    {31, 1, 2, 4, 8, 8, 8},
    {14, 17, 17, 14, 17, 17, 14},
    {14, 17, 17, 15, 1, 1, 14},
};
void loader_checkpoint(const char *text)
{
    volatile uint32_t *fb = CHECKPOINT_FB;
    const unsigned width = 1080, height = 2340, scale = 7;
    for (unsigned buffer = 0; buffer < 2; buffer++) {
        unsigned base = buffer * width * height;
        for (unsigned y = 160; y < 320; y++)
            for (unsigned x = 0; x < width; x++)
                fb[base + y * width + x] = 0x00122b52;
        for (unsigned column = 0; column < 24 && text[column]; column++) {
            unsigned c = (unsigned char)text[column], glyph;
            if (c >= 'A' && c <= 'Z') glyph = c - 'A';
            else if (c >= '0' && c <= '9') glyph = 26 + c - '0';
            else continue;
            unsigned x = 48 + column * 6 * scale;
            for (unsigned gy = 0; gy < 7; gy++)
                for (unsigned gx = 0; gx < 5; gx++)
                    if (checkpoint_font[glyph][gy] & (1u << (4 - gx)))
                        for (unsigned dy = 0; dy < scale; dy++)
                            for (unsigned dx = 0; dx < scale; dx++)
                                fb[base + (200 + gy * scale + dy) * width + x + gx * scale + dx] = 0x0000e5e5;
        }
    }
#if defined(__aarch64__)
    __asm__ volatile("dsb sy" ::: "memory");
#endif
}

static void loader_checkpoint_draw_line(volatile uint32_t *fb, unsigned base,
                                        const char *text, unsigned y)
{
    const unsigned width = 1080, scale = 7;
    for (unsigned column = 0; column < 24 && text[column]; column++) {
        unsigned c = (unsigned char)text[column], glyph;
        if (c >= 'A' && c <= 'Z') glyph = c - 'A';
        else if (c >= '0' && c <= '9') glyph = 26 + c - '0';
        else continue;
        unsigned x = 48 + column * 6 * scale;
        for (unsigned gy = 0; gy < 7; gy++)
            for (unsigned gx = 0; gx < 5; gx++)
                if (checkpoint_font[glyph][gy] & (1u << (4 - gx)))
                    for (unsigned dy = 0; dy < scale; dy++)
                        for (unsigned dx = 0; dx < scale; dx++)
                            fb[base + (y + gy * scale + dy) * width + x + gx * scale + dx] = 0x0000e5e5;
    }
}

void loader_checkpoint_hex(const char *title, uint64_t value)
{
    /* The pass path emits ADDRESS then EXTENT; avoid BSS state before relocate. */
    unsigned row = title[0] == 'S' && title[1] == 'O' && title[2] == 'U'
        && title[3] == 'R' && title[4] == 'C' && title[5] == 'E' && title[6] == ' '
        && title[7] == 'E' && title[8] == 'X';
    volatile uint32_t *fb = CHECKPOINT_FB;
    const unsigned width = 1080, height = 2340;
    for (unsigned buffer = 0; buffer < 2; buffer++) {
        unsigned base = buffer * width * height;
        if (row == 0)
            for (unsigned y = 160; y < 440; y++)
                for (unsigned x = 0; x < width; x++)
                    fb[base + y * width + x] = 0x00122b52;
        loader_checkpoint_draw_line(fb, base, title, 180 + row * 120);
    }

    static const char digits[] = "0123456789ABCDEF";
    char text[17];
    for (unsigned i = 0; i < 16; i++)
        text[i] = digits[(value >> ((15 - i) * 4)) & 0xf];
    text[16] = '\0';
    for (unsigned buffer = 0; buffer < 2; buffer++) {
        unsigned base = buffer * width * height;
        loader_checkpoint_draw_line(fb, base, text, 230 + row * 120);
    }
#if defined(__aarch64__)
    __asm__ volatile("dsb sy" ::: "memory");
#endif
}
#endif
