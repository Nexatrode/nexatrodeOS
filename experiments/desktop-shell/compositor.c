/* Landscape workspace compositor. App surfaces remain separate bounded RGB planes. */
#include "protocol.h"
#include "../../tests/input/slint_frame.h"
#include "../desktop-surfaces/input.h"
#include <microkit.h>
struct surface_header { _Atomic uint32_t revision; uint32_t width, height, stride; };
uintptr_t surface_a, surface_b, output_frame, focus_page, input_handoff, input_a, input_b;
uintptr_t shell_surface, shell_state, shell_input, ui_pointer;
static bool dynamic_active, focus_dirty;
static void dynamic_input_notify(void);
static bool dynamic_control(struct input_event_record);
static struct shell_state desktop = {.top = 2, .windows = {{68, 126, 0, 0}, {650, 110, 0, 0}}};
static uint32_t sent[3], driver_tail, driver_dropped;
static bool reset_pending[3], input_ready;
static unsigned pointer_owner;
static int32_t pointer_x, pointer_y;
static uint8_t key_owners[DESKTOP_KEY_MAX + 1];
#define APP_BYTES (APP_PIXEL_WIDTH * APP_PIXEL_HEIGHT * 3u)
static uint8_t shell_pixels[SLINT_FRAME_PIXEL_BYTES], candidate[SLINT_FRAME_PIXEL_BYTES];
static uint8_t app_pixels[2][APP_BYTES];
static uint32_t shell_revision, app_revision[2];
static bool composition_dirty;

static void publish_state(void)
{
    struct shell_state *out = (void *)shell_state;
    uint32_t revision = __atomic_load_n(&out->revision, __ATOMIC_RELAXED);
    __atomic_store_n(&out->revision, revision + 1, __ATOMIC_RELEASE);
    __atomic_thread_fence(__ATOMIC_SEQ_CST);
    out->top = desktop.top; out->focused = desktop.focused;
    out->windows[0] = desktop.windows[0]; out->windows[1] = desktop.windows[1];
    __atomic_store_n(&out->revision, revision + 2, __ATOMIC_RELEASE);
    microkit_notify(21);
    composition_dirty = true;
}
static void input_notify(unsigned owner)
{
#ifdef DESKTOP_REAL_APPS
    if (owner <= 2) { microkit_notify(owner == 1 ? 24 : 25); return; }
#endif
    if (owner == 1) { if (dynamic_active) dynamic_input_notify(); }
    else microkit_notify(owner == 2 ? 8 : 22);
}
static struct desktop_queue *queue_for(unsigned owner) { return (void *)(owner == 1 ? input_a : owner == 2 ? input_b : shell_input); }
#ifdef DESKTOP_REAL_APPS
static void app_emit(unsigned, struct desktop_event);
#endif
static void emit(unsigned owner, struct desktop_event event)
{
    if (owner < 1 || owner > 3 || (owner == 1 && !dynamic_active)) return;
#ifdef DESKTOP_REAL_APPS
    if (owner <= 2) { app_emit(owner, event); return; }
#endif
    unsigned i = owner - 1; struct desktop_queue *q = queue_for(owner);
    uint32_t tail = __atomic_load_n(&q->tail, __ATOMIC_ACQUIRE);
    if (sent[i] - tail >= DESKTOP_EVENT_CAPACITY) { reset_pending[i] = true; input_notify(owner); return; }
    if (reset_pending[i]) {
        q->events[sent[i]++ % DESKTOP_EVENT_CAPACITY] = (struct desktop_event){.kind = DESKTOP_RESET};
        __atomic_store_n(&q->head, sent[i], __ATOMIC_RELEASE); reset_pending[i] = false;
        if (sent[i] - tail >= DESKTOP_EVENT_CAPACITY) { reset_pending[i] = true; input_notify(owner); return; }
    }
    q->events[sent[i]++ % DESKTOP_EVENT_CAPACITY] = event;
    __atomic_store_n(&q->head, sent[i], __ATOMIC_RELEASE); input_notify(owner);
}
static void focus(unsigned owner)
{
    if (desktop.focused != owner) {
        if (desktop.focused) emit(desktop.focused, (struct desktop_event){.kind = DESKTOP_FOCUS});
        desktop.focused = owner;
        if (owner) emit(owner, (struct desktop_event){.kind = DESKTOP_FOCUS, .value = 1});
    }
    if (owner) desktop.top = owner;
    publish_state();
}
static void release_owner(unsigned owner)
{
    for (unsigned i = 0; i <= DESKTOP_KEY_MAX; ++i) if (key_owners[i] == owner) key_owners[i] = 0;
    if (pointer_owner == owner) pointer_owner = 0;
    emit(owner, (struct desktop_event){.kind = DESKTOP_RESET});
}
static void reset_input(void)
{
    memset(key_owners, 0, sizeof(key_owners)); pointer_owner = 0;
    for (unsigned owner = 1; owner <= 3; ++owner) emit(owner, (struct desktop_event){.kind = DESKTOP_RESET});
}
static void pointer_event(unsigned value)
{
    unsigned owner = pointer_owner ? pointer_owner : shell_hit(&desktop, pointer_x, pointer_y);
    if (value == 1 && !pointer_owner) {
        pointer_owner = owner;
        unsigned window = shell_window_at(&desktop, pointer_x, pointer_y);
        if (window) focus(window);
    }
    int32_t x = pointer_x, y = pointer_y;
    if (owner <= 2) { x -= desktop.windows[owner - 1].x; y -= desktop.windows[owner - 1].y; }
    emit(owner, (struct desktop_event){.kind = DESKTOP_POINTER, .value = value, .x = x, .y = y});
    if (value == 0) pointer_owner = 0;
}
#ifdef DESKTOP_REAL_APPS
static void app_scroll(unsigned, int32_t);
#endif
static void input(void)
{
    struct input_event_ring *ring = (void *)input_handoff;
    uint32_t head = __atomic_load_n(&ring->head, __ATOMIC_ACQUIRE);
    uint32_t dropped = __atomic_load_n(&ring->dropped, __ATOMIC_ACQUIRE);
    if (head - driver_tail > INPUT_EVENT_CAPACITY || dropped != driver_dropped) {
        reset_input(); driver_tail = head; driver_dropped = dropped;
    }
    while (driver_tail != head) {
        struct input_event_record e = ring->events[driver_tail++ & INPUT_EVENT_MASK];
        if (e.reserved) continue;
        if (e.device == INPUT_DEVICE_TABLET) {
            if (e.type == 3 && e.code <= 1 && e.value <= 32767) {
                if (!e.code) pointer_x = e.value * (SHELL_WIDTH - 1) / 32767;
                else pointer_y = e.value * (SHELL_HEIGHT - 1) / 32767;
                pointer_event(2);
            } else if (e.type == 1 && e.code == 272 && e.value <= 1) {
                pointer_event(e.value);
            }
#ifdef DESKTOP_REAL_APPS
            else if (e.type == 2 && e.code == 8 && desktop.focused) {
                unsigned owner = shell_hit(&desktop, pointer_x, pointer_y);
                if (owner <= 2) app_scroll(owner, (int32_t)e.value);
            }
#endif
        } else if (e.device == INPUT_DEVICE_KEYBOARD && e.type == 1 && e.code <= DESKTOP_KEY_MAX && e.value <= 2) {
            if (dynamic_control(e)) continue;
            unsigned owner = key_owners[e.code];
            if (e.value == 1 && !owner) owner = key_owners[e.code] = desktop.focused;
            if (owner) emit(owner, (struct desktop_event){.kind = DESKTOP_KEY, .code = e.code, .value = e.value});
            if (!e.value) key_owners[e.code] = 0;
        }
    }
    __atomic_store_n(&ring->tail, driver_tail, __ATOMIC_RELEASE);
    volatile uint32_t *pointer = (void *)ui_pointer;
    uint32_t revision = pointer[0]; pointer[0] = revision + 1; __atomic_thread_fence(__ATOMIC_SEQ_CST);
    pointer[1] = pointer_x * DESKTOP_SCALE; pointer[2] = pointer_y * DESKTOP_SCALE; pointer[3] = 1;
    __atomic_thread_fence(__ATOMIC_RELEASE); pointer[0] = revision + 2;
    microkit_notify(2);
}
static bool snapshot(uintptr_t source, unsigned width, unsigned height, uint8_t *cache, uint32_t *previous)
{
    const struct surface_header *h = (const void *)source;
    uint32_t first = atomic_load_explicit(&h->revision, memory_order_acquire);
    if (!first || (first & 1) || first == *previous || h->width != width || h->height != height || h->stride != width * 3) return false;
    const volatile uint8_t *pixels = (const void *)(h + 1);
    size_t bytes = (size_t)width * height * 3;
    for (size_t i = 0; i < bytes; ++i) candidate[i] = pixels[i];
    atomic_thread_fence(memory_order_acquire);
    if (atomic_load_explicit(&h->revision, memory_order_acquire) != first) return false;
    memcpy(cache, candidate, bytes); *previous = first; return true;
}
#ifdef DESKTOP_REAL_APPS
#include "../desktop-apps/surfaces.h"
#endif
static bool compose(void)
{
    composition_dirty |= snapshot(shell_surface, SHELL_PIXEL_WIDTH, SHELL_PIXEL_HEIGHT, shell_pixels, &shell_revision);
#ifdef DESKTOP_REAL_APPS
    composition_dirty |= snapshot_app(surface_a, app_pixels[0], &app_revision[0]);
    composition_dirty |= snapshot_app(surface_b, app_pixels[1], &app_revision[1]);
#else
    if (dynamic_active) composition_dirty |= snapshot(surface_a, APP_PIXEL_WIDTH, APP_PIXEL_HEIGHT, app_pixels[0], &app_revision[0]);
    composition_dirty |= snapshot(surface_b, APP_PIXEL_WIDTH, APP_PIXEL_HEIGHT, app_pixels[1], &app_revision[1]);
#endif
    if (!shell_revision) return false;
    if (!composition_dirty && !focus_dirty) return true;
    slint_frame_header_t *out = (void *)output_frame;
    uint32_t before = atomic_load_explicit(&out->revision, memory_order_relaxed);
    atomic_store_explicit(&out->revision, before + 1, memory_order_release); atomic_thread_fence(memory_order_seq_cst);
    uint8_t *dest = (void *)(out + 1); memcpy(dest, shell_pixels, sizeof(shell_pixels));
    unsigned order[2] = {desktop.top == 1 ? 2 : 1, desktop.top};
    for (unsigned i = 0; i < 2; ++i) {
        unsigned id = order[i]; const struct shell_window *w = &desktop.windows[id - 1];
        if (!w->visible || !app_revision[id - 1] || (id == 1 && !dynamic_active)) continue;
        /* Restore the shell-owned border/title above lower windows before
         * copying this app's content. App pixels never cover higher chrome. */
        for (int y = (w->y - 43) * DESKTOP_SCALE; y < (w->y + (int)APP_HEIGHT + 1) * (int)DESKTOP_SCALE; ++y) {
            size_t offset = (y * SHELL_PIXEL_WIDTH + (w->x - 1) * DESKTOP_SCALE) * 3;
            memcpy(dest + offset, shell_pixels + offset, (APP_WIDTH + 2) * DESKTOP_SCALE * 3);
        }
        for (unsigned y = 0; y < APP_PIXEL_HEIGHT; ++y) {
            static const unsigned corner[10] = {0,0,0,1,1,2,3,4,6,9};
            unsigned row = y / DESKTOP_SCALE;
            unsigned inset = row >= APP_HEIGHT - 10 ? corner[row - (APP_HEIGHT - 10)] * DESKTOP_SCALE : 0;
            memcpy(dest + (((w->y * DESKTOP_SCALE + y) * SHELL_PIXEL_WIDTH) + w->x * DESKTOP_SCALE + inset) * 3,
                   app_pixels[id - 1] + (y * APP_PIXEL_WIDTH + inset) * 3, (APP_PIXEL_WIDTH - 2 * inset) * 3);
        }
    }
    out->width = SHELL_PIXEL_WIDTH; out->height = SHELL_PIXEL_HEIGHT; out->stride_bytes = SHELL_PIXEL_WIDTH * 3; out->format = SLINT_FRAME_FORMAT_RGB8;
    atomic_store_explicit(&out->damage_y, 0, memory_order_relaxed); atomic_store_explicit(&out->damage_height, SHELL_PIXEL_HEIGHT, memory_order_relaxed);
    atomic_store_explicit(&out->revision, before + 2, memory_order_release);
    composition_dirty = focus_dirty = false; microkit_notify(2); return true;
}
static void __attribute__((unused)) discard_dynamic_surface(void)
{
    dynamic_active = false;
    for (unsigned i = 0; i <= DESKTOP_KEY_MAX; ++i) if (key_owners[i] == 1) key_owners[i] = 0;
    if (pointer_owner == 1) pointer_owner = 0;
    if (desktop.focused == 1) desktop.focused = 0;
    sent[0] = 0; reset_pending[0] = false; app_revision[0] = 0;
    memset(app_pixels[0], 0, sizeof(app_pixels[0])); memset(candidate, 0, sizeof(candidate));
    desktop.windows[0].open = desktop.windows[0].visible = 0;
    publish_state(); compose();
}
void init(void) { *(volatile uint32_t *)focus_page = 1; publish_state(); }
void notified(microkit_channel channel)
{
    if (channel == 6) input();
    if (compose() && !input_ready) { input_ready = true; microkit_dbg_puts("DESKTOP|WORKSPACE_READY|1280x800\n"); }
}
seL4_Bool fault(microkit_child child, microkit_msginfo info, microkit_msginfo *reply)
{ (void)child; (void)info; (void)reply; return seL4_False; }
