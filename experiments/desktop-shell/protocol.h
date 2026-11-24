/* Private shell -> trusted manager ABI; apps never receive this endpoint. */
#pragma once
#include <stdint.h>
#define SHELL_CHANNEL 61u
#define SHELL_REQUEST 0x7b0u
#define SHELL_WIDTH 1280u
#define SHELL_HEIGHT 800u
#define APP_WIDTH 560u
#define APP_HEIGHT 520u
#ifndef DESKTOP_SCALE
#define DESKTOP_SCALE 1u
#endif
#define APP_PIXEL_WIDTH (APP_WIDTH * DESKTOP_SCALE)
#define APP_PIXEL_HEIGHT (APP_HEIGHT * DESKTOP_SCALE)
#define SHELL_PIXEL_WIDTH (SHELL_WIDTH * DESKTOP_SCALE)
#define SHELL_PIXEL_HEIGHT (SHELL_HEIGHT * DESKTOP_SCALE)
_Static_assert(DESKTOP_SCALE == 1 || DESKTOP_SCALE == 2, "supported desktop scales");
#define SHELL_APP_LAUNCH 1u
#define SHELL_APP_MINIMIZE 2u
#define SHELL_APP_CLOSE 3u
#define SHELL_APP_MOVE 4u
#define SHELL_APP_FOCUS 5u
struct shell_window { int32_t x, y; uint32_t open, visible; };
struct shell_state { uint32_t revision, top, focused, reserved; struct shell_window windows[2]; };
_Static_assert(sizeof(struct shell_state) == 48, "shell state ABI");
static inline int32_t shell_clamp(int32_t value, int32_t low, int32_t high) { return value < low ? low : value > high ? high : value; }
static inline void shell_position(struct shell_window *w, int32_t x, int32_t y) {
    w->x = shell_clamp(x, 12, SHELL_WIDTH - APP_WIDTH - 12);
    w->y = shell_clamp(y, 92, SHELL_HEIGHT - APP_HEIGHT - 128);
}
static inline unsigned shell_window_at(const struct shell_state *s, int32_t x, int32_t y) {
    unsigned order[2] = {s->top == 1 ? 1 : 2, s->top == 1 ? 2 : 1};
    for (unsigned i = 0; i < 2; ++i) {
        unsigned id = order[i]; const struct shell_window *w = &s->windows[id - 1];
        if (w->visible && x >= w->x - 1 && x < w->x + (int)APP_WIDTH + 1 && y >= w->y - 43 && y < w->y + (int)APP_HEIGHT + 1)
            return id;
    }
    return 0;
}

static inline unsigned shell_hit(const struct shell_state *s, int32_t x, int32_t y) {
    unsigned id = shell_window_at(s, x, y);
    if (!id) return 3;
    const struct shell_window *w = &s->windows[id - 1];
    return y >= w->y && x >= w->x && x < w->x + (int)APP_WIDTH && y < w->y + (int)APP_HEIGHT ? id : 3;
}
