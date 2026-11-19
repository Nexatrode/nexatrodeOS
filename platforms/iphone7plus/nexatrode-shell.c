/* Draw the actual Rust app-owned view snapshots on the observed Pongo screen. */
#include <microkit.h>
#include <stdint.h>
#include "touch-state.h"

uintptr_t os_framebuffer;
uintptr_t shell_settings_view;
uintptr_t shell_phone_view;
uintptr_t shell_touch_state;
volatile unsigned int shell_boot_stage;
static int settings_ok, phone_ok, touch_was_active;

enum { STRIDE = 0x1100 / 4, WIDTH = 640, HEIGHT = 240, TEXT_CAPACITY = 128 };
typedef struct {
    volatile uint32_t revision;
    volatile uint32_t length;
    volatile uint8_t text[TEXT_CAPACITY];
} AppViewHeader;

static const char *glyph(char c)
{
    switch (c) {
    case '0': return "01110100011001110101110011000101110";
    case 'A': return "01110100011000111111100011000110001";
    case 'C': return "01111100001000010000100001000001111";
    case 'D': return "11110100011000110001100011000111110";
    case 'E': return "11111100001000011110100001000011111";
    case 'G': return "01111100001000010111100011000101111";
    case 'H': return "10001100011000111111100011000110001";
    case 'I': return "11111001000010000100001000010011111";
    case 'L': return "10000100001000010000100001000011111";
    case 'M': return "10001110111010110101100011000110001";
    case 'N': return "10001110011010110011100011000110001";
    case 'O': return "01110100011000110001100011000101110";
    case 'P': return "11110100011000111110100001000010000";
    case 'R': return "11110100011000111110101001001010001";
    case 'S': return "01111100001000001110000010000111110";
    case 'T': return "11111001000010000100001000010000100";
    case 'U': return "10001100011000110001100011000101110";
    case 'V': return "10001100011000110001100010101000100";
    case 'W': return "10001100011000110101101011101110001";
    case 'X': return "10001100010101000100010101000110001";
    case 'Y': return "10001100010101000100001000010000100";
    default: return "00000000000000000000000000000000000";
    }
}

static void draw_char(unsigned int x, unsigned int y, char c, uint32_t color)
{
    volatile uint32_t *fb = (volatile uint32_t *)os_framebuffer;
    const char *bitmap = glyph(c);
    for (unsigned int row = 0; row < 7; row++) {
        for (unsigned int column = 0; column < 5; column++) {
            if (bitmap[row * 5 + column] != '1') continue;
            for (unsigned int dy = 0; dy < 3; dy++) {
                for (unsigned int dx = 0; dx < 3; dx++) {
                    fb[(y + row * 3 + dy) * STRIDE + x + column * 3 + dx] = color;
                }
            }
        }
    }
}

static void draw_text(unsigned int x, unsigned int y, const char *text, uint32_t color)
{
    for (unsigned int i = 0; text[i] && i < 32; i++) {
        draw_char(x + i * 18, y, text[i], color);
    }
}

static void clear_screen(void)
{
    volatile uint32_t *fb = (volatile uint32_t *)os_framebuffer;
    for (unsigned int y = 0; y < HEIGHT; y++) {
        for (unsigned int x = 0; x < WIDTH; x++) {
            fb[y * STRIDE + x] = 0xff101820u;
        }
    }
}

static void clear_bottom(void)
{
    volatile uint32_t *fb = (volatile uint32_t *)os_framebuffer;
    for (unsigned int y = 216; y < HEIGHT; y++)
        for (unsigned int x = 0; x < WIDTH; x++)
            fb[y * STRIDE + x] = 0xff101820u;
}

static int read_touch(IphoneTouchState *snapshot)
{
    const volatile IphoneTouchState *state =
        (const volatile IphoneTouchState *)shell_touch_state;
    for (unsigned int attempt = 0; attempt < 4; attempt++) {
        uint32_t before = state->revision;
        if (before & 1u) continue;
        snapshot->status = state->status;
        snapshot->stage = state->stage;
        snapshot->packets = state->packets;
        snapshot->active = state->active;
        snapshot->finger = state->finger;
        snapshot->x = state->x;
        snapshot->y = state->y;
        asm volatile("dmb ish" ::: "memory");
        if (before == state->revision) return 1;
    }
    return 0;
}

static int read_view(uintptr_t address, char output[TEXT_CAPACITY + 1])
{
    const volatile AppViewHeader *view = (const volatile AppViewHeader *)address;
    for (unsigned int attempt = 0; attempt < 4; attempt++) {
        uint32_t before = view->revision;
        uint32_t length = view->length;
        if ((before & 1u) || length == 0 || length > TEXT_CAPACITY) continue;
        for (unsigned int i = 0; i < length; i++) output[i] = (char)view->text[i];
        asm volatile("dmb ish" ::: "memory");
        if (before == view->revision) {
            output[length] = 0;
            return 1;
        }
    }
    return 0;
}

static void draw_view(unsigned int x, const char *body, uint32_t color)
{
    unsigned int line = 0, column = 0;
    for (unsigned int i = 0; body[i] && line < 4; i++) {
        if (body[i] == '\n') {
            line++;
            column = 0;
        } else if (column < 16) {
            draw_char(x + column * 18, 82 + line * 34, body[i], color);
            column++;
        }
    }
}

static int set_alerts(microkit_channel channel, seL4_Word request, seL4_Word expected_reply)
{
    microkit_mr_set(0, 0);
    microkit_mr_set(1, 0);
    microkit_msginfo reply = microkit_ppcall(channel, microkit_msginfo_new(request, 2));
    return microkit_msginfo_get_label(reply) == expected_reply &&
           microkit_msginfo_get_count(reply) == 0;
}

static void draw_selected(unsigned int selected)
{
    char body[TEXT_CAPACITY + 1];
    clear_screen();
    draw_text(24, 12, "NEXATRODE OS", 0xffffffffu);
    if (selected == 1 && settings_ok &&
        read_view(shell_settings_view, body)) {
        draw_text(24, 48, "SETTINGS", 0xffff00ffu);
        draw_view(24, body, 0xffffffffu);
    } else if (selected == 2 && phone_ok &&
               read_view(shell_phone_view, body)) {
        draw_text(24, 48, "PHONE DEMO", 0xffffff00u);
        draw_view(24, body, 0xffffffffu);
    } else {
        draw_text(24, 82, "APP FAIL", 0xffff0000u);
    }
    draw_text(24, 216, "TOUCH READY", 0xff00ff00u);
    shell_boot_stage = 6;
}

void init(void)
{
    clear_screen();
    draw_text(24, 12, "NEXATRODE OS", 0xffffffffu);
    asm volatile("dsb sy" ::: "memory");
    shell_boot_stage = 1;

    settings_ok = set_alerts(0, 0x601, 0x600);
    shell_boot_stage = 2;
    phone_ok = set_alerts(1, 0x701, 0x700);
    shell_boot_stage = 3;

    char settings[TEXT_CAPACITY + 1], phone[TEXT_CAPACITY + 1];
    if (settings_ok && phone_ok &&
        read_view(shell_settings_view, settings) &&
        read_view(shell_phone_view, phone)) {
        draw_text(24, 48, "SETTINGS", 0xffff00ffu);
        draw_text(330, 48, "PHONE DEMO", 0xffffff00u);
        draw_view(24, settings, 0xffffffffu);
        draw_view(330, phone, 0xffffffffu);
        shell_boot_stage = 4;
    } else {
        draw_text(24, 82, "APP FAIL", 0xffff0000u);
        shell_boot_stage = 5;
    }
    asm volatile("dsb sy" ::: "memory");
}

void notified(microkit_channel channel)
{
    IphoneTouchState touch;
    if (channel != 2 || !read_touch(&touch)) return;
    if (touch.status == IPHONE_TOUCH_ERROR) {
        clear_bottom();
        draw_text(24, 216, "TOUCH FAIL", 0xffff0000u);
    } else if (touch.status == IPHONE_TOUCH_READY) {
        if (touch.active && !touch_was_active)
            draw_selected(touch.x < 2048 ? 1u : 2u);
        else if (shell_boot_stage != 6) {
            clear_bottom();
            draw_text(24, 216, "TOUCH READY", 0xff00ff00u);
        }
        touch_was_active = !!touch.active;
    }
    asm volatile("dsb sy" ::: "memory");
}
