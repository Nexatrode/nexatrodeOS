/* Experimental bounded input ABI. Only the compositor writes event payloads. */
#pragma once
#include <stdbool.h>
#include <stdint.h>
#include "../../tests/input/input_handoff.h"

#define DESKTOP_EVENT_CAPACITY 64u
#define DESKTOP_KEY 1u
#define DESKTOP_POINTER 2u
#define DESKTOP_FOCUS 3u
#define DESKTOP_RESET 4u
#define DESKTOP_KEY_MAX 767u
struct desktop_event { uint32_t kind, code, value; int32_t x, y; };
struct desktop_queue {
    uint32_t head, tail, reserved[2];
    struct desktop_event events[DESKTOP_EVENT_CAPACITY];
};
struct desktop_router {
    unsigned focus, pointer_owner;
    uint32_t x, y;
    uint8_t keys[DESKTOP_KEY_MAX + 1];
    bool tab_down;
};
typedef void (*desktop_emit)(unsigned owner, struct desktop_event event);

static inline unsigned desktop_hit(uint32_t x, uint32_t y)
{
    if (y < 500 || y >= 1300) return 0;
    if (x >= 40 && x < 520) return 1;
    if (x >= 560 && x < 1040) return 2;
    return 0;
}
static inline void desktop_focus(struct desktop_router *router, unsigned focus, desktop_emit emit)
{
    if (focus == router->focus) return;
    if (router->focus) emit(router->focus, (struct desktop_event){.kind = DESKTOP_FOCUS});
    router->focus = focus;
    if (focus) emit(focus, (struct desktop_event){.kind = DESKTOP_FOCUS, .value = 1});
}
static inline void desktop_reset(struct desktop_router *router, desktop_emit emit)
{
    for (unsigned i = 0; i <= DESKTOP_KEY_MAX; ++i) router->keys[i] = 0;
    router->pointer_owner = 0;
    router->tab_down = false;
    emit(1, (struct desktop_event){.kind = DESKTOP_RESET});
    emit(2, (struct desktop_event){.kind = DESKTOP_RESET});
}
static inline void desktop_route(struct desktop_router *router, struct input_event_record event,
                                  desktop_emit emit)
{
    if (event.reserved) return;
    if (event.device == INPUT_DEVICE_TABLET) {
        if (event.type == 3 && event.code <= 1 && event.value <= 32767) {
            if (event.code == 0) router->x = event.value * 1079 / 32767;
            else router->y = event.value * 2339 / 32767;
            unsigned owner = router->pointer_owner ? router->pointer_owner : desktop_hit(router->x, router->y);
            if (owner) emit(owner, (struct desktop_event){.kind = DESKTOP_POINTER, .value = 2,
                .x = (int32_t)router->x - (owner == 1 ? 40 : 560), .y = (int32_t)router->y - 500});
        } else if (event.type == 1 && event.code == 272 && event.value <= 1) {
            if (event.value == 1 && !router->pointer_owner) {
                unsigned owner = desktop_hit(router->x, router->y);
                desktop_focus(router, owner, emit);
                router->pointer_owner = owner;
            }
            unsigned owner = router->pointer_owner;
            if (owner) emit(owner, (struct desktop_event){.kind = DESKTOP_POINTER, .value = event.value,
                .x = (int32_t)router->x - (owner == 1 ? 40 : 560), .y = (int32_t)router->y - 500});
            if (!event.value) router->pointer_owner = 0;
        }
    } else if (event.device == INPUT_DEVICE_KEYBOARD && event.type == 1 &&
               event.code <= DESKTOP_KEY_MAX && event.value <= 2) {
        if (event.code == 15) {
            if (event.value == 1 && !router->tab_down)
                desktop_focus(router, router->focus == 1 ? 2 : 1, emit);
            router->tab_down = event.value != 0;
            return;
        }
        unsigned owner = router->keys[event.code];
        if (event.value == 1 && !owner) owner = router->keys[event.code] = router->focus;
        if (owner) emit(owner, (struct desktop_event){.kind = DESKTOP_KEY,
            .code = event.code, .value = event.value});
        if (!event.value) router->keys[event.code] = 0;
    }
}

_Static_assert(sizeof(struct desktop_event) == 20, "desktop input ABI");
_Static_assert(sizeof(struct desktop_queue) <= 4096, "one input page per app");
