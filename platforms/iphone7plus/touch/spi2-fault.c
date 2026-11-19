/* On-screen fault monitor for isolated D11 touch diagnostics. */
#include <microkit.h>
#include <stdint.h>

uintptr_t spi2_fault_framebuffer;

enum { STRIDE = 0x1100 / 4, WIDTH = 640 };

static const char *glyph(char c)
{
    switch (c) {
    case '0': return "01110100011001110101110011000101110";
    case '1': return "00100011000010000100001000010001110";
    case '2': return "01110100010000100010001000100011111";
    case '3': return "11110000010000101110000010000111110";
    case '4': return "00010001100101010010111110001000010";
    case '5': return "11111100001000011110000010000111110";
    case '6': return "01110100001000011110100011000101110";
    case '7': return "11111000010001000100010000100001000";
    case '8': return "01110100011000101110100011000101110";
    case '9': return "01110100011000101111000010000101110";
    case 'A': return "01110100011000111111100011000110001";
    case 'B': return "11110100011000111110100011000111110";
    case 'C': return "01111100001000010000100001000001111";
    case 'D': return "11110100011000110001100011000111110";
    case 'E': return "11111100001000011110100001000011111";
    case 'F': return "11111100001000011110100001000010000";
    case 'L': return "10000100001000010000100001000011111";
    case 'R': return "11110100011000111110101001001010001";
    case 'S': return "01111100001000001110000010000111110";
    case 'T': return "11111001000010000100001000010000100";
    case 'U': return "10001100011000110001010100010000100";
    default: return "00000000000000000000000000000000000";
    }
}

static void draw_char(unsigned x, unsigned y, char c, uint32_t color)
{
    volatile uint32_t *fb = (volatile uint32_t *)spi2_fault_framebuffer;
    const char *bits = glyph(c);
    for (unsigned row = 0; row < 7; row++)
        for (unsigned col = 0; col < 5; col++)
            if (bits[row * 5 + col] == '1')
                for (unsigned dy = 0; dy < 2; dy++)
                    for (unsigned dx = 0; dx < 2; dx++)
                        fb[(y + row * 2 + dy) * STRIDE + x + col * 2 + dx] = color;
}

static void draw_text(unsigned x, unsigned y, const char *text)
{
    for (unsigned i = 0; text[i]; i++)
        draw_char(x + i * 12, y, text[i], 0xffffffffu);
}

static void draw_hex(unsigned x, unsigned y, uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (unsigned i = 0; i < 8; i++)
        draw_char(x + i * 12, y, digits[(value >> (28 - i * 4)) & 15], 0xffffffffu);
}

static void draw_hex64(unsigned x, unsigned y, uint64_t value)
{
    draw_hex(x, y, (uint32_t)(value >> 32));
    draw_hex(x + 8 * 12, y, (uint32_t)value);
}

void init(void) {}
void notified(microkit_channel channel) { (void)channel; }

seL4_Bool fault(microkit_child child, microkit_msginfo msginfo,
                microkit_msginfo *reply_msginfo)
{
    (void)child;
    (void)reply_msginfo;
    volatile uint32_t *fb = (volatile uint32_t *)spi2_fault_framebuffer;
    /* Clear the probe's result rows so the fault address stays readable. */
    for (unsigned y = 0; y < 145; y++)
        for (unsigned x = 0; x < WIDTH; x++)
            fb[y * STRIDE + x] = 0xff101820u;
    for (unsigned y = 0; y < 22; y++)
        for (unsigned x = 0; x < WIDTH; x++)
            fb[y * STRIDE + x] = 0xffaa0000u;
    draw_text(20, 4, "FAULT");
    draw_text(180, 4, "ERR");
    draw_hex(250, 4, (uint32_t)microkit_msginfo_get_label(msginfo));
    if (microkit_msginfo_get_label(msginfo) == seL4_Fault_VMFault) {
        draw_text(20, 40, "ADDR");
        draw_hex64(100, 40, (uint64_t)seL4_GetMR(seL4_VMFault_Addr));
        draw_text(20, 70, "FSR");
        draw_hex(100, 70, (uint32_t)seL4_GetMR(seL4_VMFault_FSR));
    } else if (microkit_msginfo_get_label(msginfo) == seL4_Fault_UserException) {
        draw_text(20, 40, "IP");
        draw_hex64(100, 40, (uint64_t)seL4_GetMR(seL4_UserException_FaultIP));
        draw_text(20, 70, "NUM");
        draw_hex(100, 70, (uint32_t)seL4_GetMR(seL4_UserException_Number));
    }
    return seL4_False;
}
