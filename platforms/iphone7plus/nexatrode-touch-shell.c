/* Full-screen iPhone shell: D11 touch selects and drives Rust-owned views. */
#include <microkit.h>
#include <stdint.h>
#include "touch-state.h"

uintptr_t os_framebuffer;
uintptr_t shell_settings_view;
uintptr_t shell_phone_view;
uintptr_t shell_touch_state;

enum {
    WIDTH = 1080, HEIGHT = 1920, STRIDE = 0x1100 / 4,
    TEXT_CAPACITY = 128, TOUCH_MAX = 4096,
    HOME = 0, SETTINGS = 1, PHONE = 2,
    PHONE_INPUT_CHANNEL = 3, REQUEST_ROUTED_EVENT = 0x310,
    REPLY_ROUTED_EVENT_OK = 0x311, DEVICE_KEYBOARD = 0, EVENT_KEY = 1,
    KEY_CALL = 63, KEY_CLEAR = 64, KEY_BACKSPACE = 65
};

typedef struct {
    volatile uint32_t revision;
    volatile uint32_t length;
    volatile uint8_t text[TEXT_CAPACITY];
} AppViewHeader;

static unsigned int selected = HOME;
static unsigned int touch_ready;
static unsigned int touch_failed;
static unsigned int touch_was_active;
static unsigned int settings_ok;
static unsigned int phone_ok;
static uint32_t input_sequence;

static const char *glyph(char c)
{
    switch (c) {
    case '0': return "01110100011001110101110011000101110";
    case '1': return "00100011000010000100001000010001110";
    case '2': return "01110100010000100010001000100011111";
    case '3': return "11110000010000101110000010000111110";
    case '4': return "00010001100101010010111110001000010";
    case '5': return "11111100001111000001000011000101110";
    case '6': return "00110010001000011110100011000101110";
    case '7': return "11111000010001000100010000100001000";
    case '8': return "01110100011000101110100011000101110";
    case '9': return "01110100011000101111000010001001100";
    case 'A': return "01110100011000111111100011000110001";
    case 'B': return "11110100011000111110100011000111110";
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

static void fill_rect(unsigned x, unsigned y, unsigned width, unsigned height,
                      uint32_t color)
{
    volatile uint32_t *fb = (volatile uint32_t *)os_framebuffer;
    if (x >= WIDTH || y >= HEIGHT) return;
    if (width > WIDTH - x) width = WIDTH - x;
    if (height > HEIGHT - y) height = HEIGHT - y;
    for (unsigned row = 0; row < height; row++)
        for (unsigned col = 0; col < width; col++)
            fb[(y + row) * STRIDE + x + col] = color;
}

static void draw_char(unsigned x, unsigned y, char c, unsigned scale,
                      uint32_t color)
{
    const char *bitmap = glyph(c);
    for (unsigned row = 0; row < 7; row++)
        for (unsigned col = 0; col < 5; col++)
            if (bitmap[row * 5 + col] == '1')
                fill_rect(x + col * scale, y + row * scale,
                          scale, scale, color);
}

static void draw_text(unsigned x, unsigned y, const char *text, unsigned scale,
                      uint32_t color)
{
    for (unsigned i = 0; text[i] && i < 32; i++)
        draw_char(x + i * 6 * scale, y, text[i], scale, color);
}

static int read_view(uintptr_t address, char output[TEXT_CAPACITY + 1])
{
    const volatile AppViewHeader *view = (const volatile AppViewHeader *)address;
    for (unsigned attempt = 0; attempt < 4; attempt++) {
        uint32_t before = view->revision;
        uint32_t length = view->length;
        if ((before & 1u) || !length || length > TEXT_CAPACITY) continue;
        for (unsigned i = 0; i < length; i++) output[i] = (char)view->text[i];
        asm volatile("dmb ish" ::: "memory");
        if (before == view->revision) {
            output[length] = 0;
            return 1;
        }
    }
    return 0;
}

static int read_touch(IphoneTouchState *snapshot)
{
    const volatile IphoneTouchState *state =
        (const volatile IphoneTouchState *)shell_touch_state;
    for (unsigned attempt = 0; attempt < 4; attempt++) {
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

static void draw_view(uintptr_t address)
{
    char body[TEXT_CAPACITY + 1];
    if (!read_view(address, body)) {
        draw_text(70, 220, "APP FAIL", 8, 0xffff0000u);
        return;
    }
    unsigned line = 0, column = 0;
    for (unsigned i = 0; body[i] && line < 4; i++) {
        if (body[i] == '\n') {
            line++;
            column = 0;
        } else if (column < 18) {
            draw_char(70 + column * 42, 220 + line * 82,
                      body[i], 7, 0xffffffffu);
            column++;
        }
    }
}

static void draw_header(const char *title)
{
    fill_rect(0, 0, WIDTH, HEIGHT, 0xff101820u);
    fill_rect(0, 0, WIDTH, 140, 0xff702028u);
    draw_text(60, 38, title, 10, 0xffffffffu);
}

static void draw_status(void)
{
    fill_rect(0, 1820, WIDTH, 100, 0xff101820u);
    if (touch_failed)
        draw_text(60, 1840, "TOUCH FAIL", 7, 0xffff0000u);
    else if (touch_ready)
        draw_text(60, 1840, "TOUCH READY", 7, 0xff00ff00u);
    else
        draw_text(60, 1840, "TOUCH WAIT", 7, 0xffffff00u);
}

static void draw_button(unsigned x, unsigned y, unsigned width, unsigned height,
                        const char *label, uint32_t color)
{
    fill_rect(x, y, width, height, color);
    fill_rect(x + 6, y + 6, width - 12, height - 12, 0xff202830u);
    unsigned length = 0;
    while (label[length]) length++;
    unsigned scale = length > 5 ? 7 : 9;
    unsigned text_width = length * 6 * scale;
    draw_text(x + (width - text_width) / 2,
              y + (height - 7 * scale) / 2, label, scale, color);
}

static void draw_home_button(void)
{
    draw_button(60, 1680, 960, 120, "HOME", 0xff50b0ffu);
}

static void render_home(void)
{
    selected = HOME;
    draw_header("NEXATRODE OS");
    draw_text(60, 235, "TOUCH AN APP", 8, 0xffffffffu);
    draw_button(60, 430, 460, 900, "SETTINGS", 0xffff00ffu);
    draw_button(560, 430, 460, 900, "PHONE", 0xffffff00u);
    draw_status();
}

static void render_settings(void)
{
    selected = SETTINGS;
    draw_header("SETTINGS");
    if (settings_ok) draw_view(shell_settings_view);
    else draw_text(70, 220, "APP FAIL", 8, 0xffff0000u);
    draw_home_button();
    draw_status();
}

static void render_phone(void)
{
    selected = PHONE;
    draw_header("PHONE DEMO");
    if (phone_ok) draw_view(shell_phone_view);
    else draw_text(70, 220, "APP FAIL", 8, 0xffff0000u);
    static const char *keys[4][3] = {
        {"1", "2", "3"}, {"4", "5", "6"},
        {"7", "8", "9"}, {"CLR", "0", "DEL"}
    };
    for (unsigned row = 0; row < 4; row++)
        for (unsigned col = 0; col < 3; col++)
            draw_button(60 + col * 340, 590 + row * 260, 300, 210,
                        keys[row][col], 0xff50b0ffu);
    draw_button(60, 1585, 960, 85, "CALL DEMO", 0xff00ff00u);
    draw_home_button();
    draw_status();
}

static void redraw_phone_view(void)
{
    fill_rect(0, 160, WIDTH, 380, 0xff101820u);
    draw_view(shell_phone_view);
}

static int set_alerts(microkit_channel channel, seL4_Word request,
                      seL4_Word expected_reply)
{
    microkit_mr_set(0, 0);
    microkit_mr_set(1, 0);
    microkit_msginfo reply = microkit_ppcall(channel,
                                             microkit_msginfo_new(request, 2));
    return microkit_msginfo_get_label(reply) == expected_reply &&
           microkit_msginfo_get_count(reply) == 0;
}

static int phone_key(unsigned code)
{
    input_sequence++;
    microkit_mr_set(0, ((seL4_Word)DEVICE_KEYBOARD << 32) |
                       ((seL4_Word)EVENT_KEY << 16) | code);
    microkit_mr_set(1, 1);
    microkit_mr_set(2, input_sequence);
    microkit_msginfo reply = microkit_ppcall(PHONE_INPUT_CHANNEL,
                             microkit_msginfo_new(REQUEST_ROUTED_EVENT, 3));
    return microkit_msginfo_get_label(reply) == REPLY_ROUTED_EVENT_OK &&
           microkit_msginfo_get_count(reply) == 0;
}

static unsigned phone_code(unsigned x, unsigned y)
{
    if (x < 60 || x >= 1020 || y < 590 || y >= 1580) return 0;
    unsigned column = (x - 60) / 340;
    unsigned row = (y - 590) / 260;
    if (column >= 3 || row >= 4 || (x - 60) % 340 >= 300 ||
        (y - 590) % 260 >= 210) return 0;
    static const unsigned codes[4][3] = {
        {2, 3, 4}, {5, 6, 7}, {8, 9, 10},
        {KEY_CLEAR, 11, KEY_BACKSPACE}
    };
    return codes[row][column];
}

static void handle_press(uint32_t raw_x, uint32_t raw_y)
{
    unsigned x = (uint64_t)raw_x * WIDTH / TOUCH_MAX;
    unsigned y = (uint64_t)raw_y * HEIGHT / TOUCH_MAX;
    if (x >= WIDTH) x = WIDTH - 1;
    if (y >= HEIGHT) y = HEIGHT - 1;
    if (selected == HOME) {
        if (y >= 430 && y < 1330) {
            if (x >= 60 && x < 520) render_settings();
            else if (x >= 560 && x < 1020) render_phone();
        }
        return;
    }
    if (y >= 1680 && y < 1800 && x >= 60 && x < 1020) {
        render_home();
        return;
    }
    if (selected == PHONE && phone_ok) {
        unsigned code = y >= 1585 && y < 1670 && x >= 60 && x < 1020
            ? KEY_CALL : phone_code(x, y);
        if (code && phone_key(code)) redraw_phone_view();
    }
}

void init(void)
{
    settings_ok = set_alerts(0, 0x601, 0x600);
    phone_ok = set_alerts(1, 0x701, 0x700);
    render_home();
    asm volatile("dsb sy" ::: "memory");
}

void notified(microkit_channel channel)
{
    IphoneTouchState touch;
    if (channel != 2 || !read_touch(&touch)) return;
    if (touch.status == IPHONE_TOUCH_ERROR) {
        touch_failed = 1;
        draw_status();
    } else if (touch.status == IPHONE_TOUCH_READY) {
        if (!touch_ready) {
            touch_ready = 1;
            draw_status();
        }
        if (touch.active && !touch_was_active)
            handle_press(touch.x, touch.y);
        touch_was_active = !!touch.active;
    }
    asm volatile("dsb sy" ::: "memory");
}
