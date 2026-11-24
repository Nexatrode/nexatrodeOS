#include <assert.h>
#include <stdio.h>
#include <string.h>
#define SURFACE_INTERACTIVE
#define SURFACE_DYNAMIC
#include "../experiments/desktop-surfaces/compositor.c"
static unsigned input_notifications;
static void dynamic_input_notify(void) { ++input_notifications; }
static bool dynamic_control(struct input_event_record event) { (void)event; return false; }
struct fixture_surface { struct surface_header header; uint8_t pixels[APP_BYTES]; };
static struct fixture_surface first, second;
static _Alignas(8) uint8_t output[SLINT_FRAME_REGION_BYTES];
static struct desktop_queue first_input, second_input;
static uint32_t focus;

static void offer(struct fixture_surface *surface, uint8_t colour)
{
    surface->header.width = APP_WIDTH; surface->header.height = APP_HEIGHT;
    surface->header.stride = APP_WIDTH * 3;
    memset(surface->pixels, colour, sizeof(surface->pixels));
    atomic_store(&surface->header.revision, 2);
}
static uint8_t visible(unsigned x)
{
    return output[sizeof(slint_frame_header_t) + (500 * SLINT_FRAME_WIDTH + x) * 3];
}
int main(void)
{
    surface_b = (uintptr_t)&second; /* A starts unmapped. */
    input_b = (uintptr_t)&second_input;
    output_frame = (uintptr_t)output; focus_page = (uintptr_t)&focus;
    offer(&second, 42); init();
    assert(compose() && visible(40) == 4 && visible(560) == 42);
    emit(1, (struct desktop_event){.kind = DESKTOP_KEY}); /* Must not dereference its absent queue. */
    assert(input_notifications == 0);
    surface_a = (uintptr_t)&first; input_a = (uintptr_t)&first_input;
    dynamic_active = true;
    offer(&first, 99); assert(compose());
    assert(visible(40) == 99);
    router.keys[30] = 1; router.keys[48] = 2;
    router.focus = 1; router.pointer_owner = 1;
    emit(1, (struct desktop_event){.kind = DESKTOP_KEY, .code = 30, .value = 1});
    assert(input_notifications == 1);
    reset_pending[0] = true;
    discard_dynamic_surface();
    assert(!dynamic_active && !router.focus && !router.pointer_owner);
    assert(!router.keys[30] && router.keys[48] == 2);
    assert(!sent[0] && !reset_pending[0] && !cached_revisions[0]);
    for (size_t i = 0; i < sizeof(cached_pixels[0]); ++i) assert(!cached_pixels[0][i]);
    for (size_t i = 0; i < sizeof(candidate_pixels); ++i) assert(!candidate_pixels[i]);
    assert(visible(40) == 4 && visible(560) == 42);
    surface_a = input_a = 0; /* Simulate the revoked address-space mappings. */
    assert(compose());
    emit(1, (struct desktop_event){.kind = DESKTOP_KEY});
    assert(input_notifications == 1);
    /* The new instance's first revision must not be confused with the old one. */
    memset(&first_input, 0, sizeof(first_input));
    surface_a = (uintptr_t)&first; input_a = (uintptr_t)&first_input;
    offer(&first, 7); dynamic_active = true; focus_dirty = true;
    assert(compose() && visible(40) == 7 && visible(560) == 42);
    emit(1, (struct desktop_event){.kind = DESKTOP_KEY, .code = 32, .value = 1});
    assert(first_input.head == 1 && first_input.events[0].code == 32);
    puts("PASS unmapped window, cache/input teardown, peer retention and fresh first revision");
}
