/* Private shell -> trusted manager ABI; apps never receive this endpoint. */
#pragma once
#include <stdint.h>
#define SHELL_CHANNEL 61u
#define SHELL_REQUEST 0x7b0u
#ifdef DESKTOP_DYNAMIC
static uint32_t desktop_width=1280u,desktop_height=800u,desktop_app_height=520u;
#define SHELL_WIDTH desktop_width
#define SHELL_HEIGHT desktop_height
#else
#define SHELL_WIDTH 1280u
#define SHELL_HEIGHT 800u
#endif
#ifndef DESKTOP_APP_COUNT
#define DESKTOP_APP_COUNT 2u
#endif
#define DESKTOP_SHELL_OWNER (DESKTOP_APP_COUNT + 1u)
_Static_assert(DESKTOP_APP_COUNT == 2 || DESKTOP_APP_COUNT == 3, "supported app counts");
#define APP_WIDTH 560u
#define APP_MAX_HEIGHT 520u
#ifdef DESKTOP_DYNAMIC
#define APP_HEIGHT desktop_app_height
#else
#define APP_HEIGHT APP_MAX_HEIGHT
#endif
#ifndef DESKTOP_SCALE
#define DESKTOP_SCALE 1u
#endif
#define APP_PIXEL_WIDTH (APP_WIDTH * DESKTOP_SCALE)
#define APP_PIXEL_HEIGHT (APP_HEIGHT * DESKTOP_SCALE)
#define APP_MAX_PIXEL_HEIGHT (APP_MAX_HEIGHT * DESKTOP_SCALE)
#define SHELL_PIXEL_WIDTH (SHELL_WIDTH * DESKTOP_SCALE)
#define SHELL_PIXEL_HEIGHT (SHELL_HEIGHT * DESKTOP_SCALE)
_Static_assert(DESKTOP_SCALE == 1 || DESKTOP_SCALE == 2, "supported desktop scales");
#define SHELL_APP_LAUNCH 1u
#define SHELL_APP_MINIMIZE 2u
#define SHELL_APP_CLOSE 3u
#define SHELL_APP_MOVE 4u
#define SHELL_APP_FOCUS 5u
struct shell_window { int32_t x, y; uint32_t open, visible; };
struct shell_state { uint32_t revision, top, focused, reserved; struct shell_window windows[DESKTOP_APP_COUNT]; };
_Static_assert(sizeof(struct shell_state) == 16 + 16 * DESKTOP_APP_COUNT, "shell state ABI");
static inline int32_t shell_clamp(int32_t value, int32_t low, int32_t high) { return value < low ? low : value > high ? high : value; }
static inline void shell_position(struct shell_window *w, int32_t x, int32_t y) {
    w->x = shell_clamp(x, 12, SHELL_WIDTH - APP_WIDTH - 12);
    w->y = shell_clamp(y, 92, SHELL_HEIGHT - APP_HEIGHT - 128);
}
/* Bottom-to-top order; the reserved ABI word carries three packed IDs only
 * in the opt-in three-window image. The original two-window ABI is unchanged. */
static inline void shell_order(const struct shell_state *s, unsigned order[DESKTOP_APP_COUNT]) {
#if DESKTOP_APP_COUNT == 3
    uint32_t packed = s->reserved ? s->reserved : 0x321;
    for (unsigned i=0;i<DESKTOP_APP_COUNT;++i) order[i]=(packed>>(4*i))&15;
#else
    order[0]=s->top==1?2:1; order[1]=s->top;
#endif
}
static inline void shell_raise(struct shell_state *s, unsigned id) {
#if DESKTOP_APP_COUNT == 3
    unsigned order[DESKTOP_APP_COUNT], next=0; shell_order(s,order);
    s->reserved=0;
    for(unsigned i=0;i<DESKTOP_APP_COUNT;++i)if(order[i]!=id)s->reserved|=order[i]<<(4*next++);
    s->reserved|=id<<(4*next);
#endif
    s->top=id;
}
static inline unsigned shell_window_at(const struct shell_state *s, int32_t x, int32_t y) {
    unsigned order[DESKTOP_APP_COUNT]; shell_order(s,order);
    for (unsigned i = DESKTOP_APP_COUNT; i != 0; --i) {
        unsigned id = order[i-1]; const struct shell_window *w = &s->windows[id - 1];
        if (w->visible && x >= w->x - 1 && x < w->x + (int)APP_WIDTH + 1 && y >= w->y - 43 && y < w->y + (int)APP_HEIGHT + 1)
            return id;
    }
    return 0;
}

static inline unsigned shell_hit(const struct shell_state *s, int32_t x, int32_t y) {
    unsigned id = shell_window_at(s, x, y);
    if (!id) return DESKTOP_SHELL_OWNER;
    const struct shell_window *w = &s->windows[id - 1];
    return y >= w->y && x >= w->x && x < w->x + (int)APP_WIDTH && y < w->y + (int)APP_HEIGHT ? id : DESKTOP_SHELL_OWNER;
}
