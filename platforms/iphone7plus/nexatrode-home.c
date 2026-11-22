/* First iPhone NexatrodeOS client: draw a boot banner and call the real Rust service. */
#include "protocol.h"
#include <stdint.h>

uintptr_t os_framebuffer;
volatile unsigned int os_boot_stage;

enum { FB_STRIDE_PIXELS = 0x1100 / 4, FB_WIDTH = 640, FB_HEIGHT = 240 };

static const char *glyph(char c)
{
    switch (c) {
    case '2': return "01110100010000100010001000100011111";
    case '4': return "00010001100101010010111110001000010";
    case 'A': return "01110100011000111111100011000110001";
    case 'C': return "01111100001000010000100001000001111";
    case 'D': return "11110100011000110001100011000111110";
    case 'E': return "11111100001000011110100001000011111";
    case 'F': return "11111100001000011110100001000010000";
    case 'I': return "11111001000010000100001000010011111";
    case 'L': return "10000100001000010000100001000011111";
    case 'N': return "10001110011010110011100011000110001";
    case 'O': return "01110100011000110001100011000101110";
    case 'P': return "11110100011000111110100001000010000";
    case 'R': return "11110100011000111110101001001010001";
    case 'S': return "01111100001000001110000010000111110";
    case 'T': return "11111001000010000100001000010000100";
    case 'U': return "10001100011000110001100011000101110";
    case 'X': return "10001100010101000100010101000110001";
    default: return "00000000000000000000000000000000000";
    }
}

static void clear_screen(void)
{
    volatile uint32_t *fb = (volatile uint32_t *)os_framebuffer;
    for (unsigned int y = 0; y < FB_HEIGHT; y++) {
        for (unsigned int x = 0; x < FB_WIDTH; x++) {
            fb[y * FB_STRIDE_PIXELS + x] = 0xff101820u;
        }
    }
}

static void draw_text(unsigned int y, const char *text, uint32_t color)
{
    volatile uint32_t *fb = (volatile uint32_t *)os_framebuffer;
    for (unsigned int i = 0; text[i]; i++) {
        const char *bitmap = glyph(text[i]);
        for (unsigned int row = 0; row < 7; row++) {
            for (unsigned int column = 0; column < 5; column++) {
                if (bitmap[row * 5 + column] != '1') continue;
                for (unsigned int dy = 0; dy < 8; dy++) {
                    for (unsigned int dx = 0; dx < 8; dx++) {
                        fb[(y + row * 8 + dy) * FB_STRIDE_PIXELS +
                           24 + i * 48 + column * 8 + dx] = color;
                    }
                }
            }
        }
    }
}

void init(void)
{
    clear_screen();
    draw_text(24, "NEXATRODE OS", 0xffffffffu);
    draw_text(104, "RUST", 0xffffff00u);
    asm volatile("dsb sy" ::: "memory");
    os_boot_stage = 1;

    microkit_mr_set(0, 19);
    microkit_mr_set(1, 23);
    microkit_msginfo reply = microkit_ppcall(
        SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 2));
    if (microkit_msginfo_get_label(reply) == REPLY_OK &&
        microkit_msginfo_get_count(reply) == 1 && microkit_mr_get(0) == 42) {
        draw_text(184, "CALC 42", 0xff00ff00u);
        os_boot_stage = 2;
    } else {
        draw_text(184, "IPC FAIL", 0xffff0000u);
        os_boot_stage = 3;
    }
    asm volatile("dsb sy" ::: "memory");
}

void notified(microkit_channel channel) { (void)channel; }
