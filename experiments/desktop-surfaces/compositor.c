#include "../../tests/input/slint_frame.h"
#include <microkit.h>
#ifdef SURFACE_INTERACTIVE
#include "input.h"
#ifdef SURFACE_DYNAMIC
static bool dynamic_active;
static void dynamic_input_notify(void);
static bool dynamic_control(struct input_event_record event);
#endif
uintptr_t input_handoff, input_a, input_b;
static struct desktop_router router;
static uint32_t sent[2], driver_tail, driver_dropped;
static bool reset_pending[2];
static bool focus_dirty, input_ready;

static void input_notify(unsigned owner)
{
#ifdef SURFACE_DYNAMIC
    if (owner == 1) { if (dynamic_active) dynamic_input_notify(); return; }
#endif
    microkit_notify(owner + 6);
}

static struct desktop_queue *queue_for(unsigned owner)
{
    return (void *)(owner == 1 ? input_a : input_b);
}

static void flush_reset(unsigned owner)
{
    if (!reset_pending[owner - 1]) return;
    struct desktop_queue *queue = queue_for(owner);
    uint32_t *head = &sent[owner - 1];
    uint32_t tail = __atomic_load_n(&queue->tail, __ATOMIC_ACQUIRE);
    if (*head - tail >= DESKTOP_EVENT_CAPACITY) return;
    queue->events[(*head)++ % DESKTOP_EVENT_CAPACITY] =
        (struct desktop_event){.kind = DESKTOP_RESET};
    __atomic_store_n(&queue->head, *head, __ATOMIC_RELEASE);
    reset_pending[owner - 1] = false;
    input_notify(owner);
}

static void emit(unsigned owner, struct desktop_event event)
{
    if (owner < 1 || owner > 2) return;
#ifdef SURFACE_DYNAMIC
    if (owner == 1 && !dynamic_active) return;
#endif
    flush_reset(owner);
    struct desktop_queue *queue = queue_for(owner);
    uint32_t *head = &sent[owner - 1];
    uint32_t tail = __atomic_load_n(&queue->tail, __ATOMIC_ACQUIRE);
    if (*head - tail >= DESKTOP_EVENT_CAPACITY) {
        /* Never overwrite unread events or modify the consumer's tail. */
        reset_pending[owner - 1] = true;
        input_notify(owner);
        return;
    }
    queue->events[(*head)++ % DESKTOP_EVENT_CAPACITY] = event;
    __atomic_store_n(&queue->head, *head, __ATOMIC_RELEASE);
    input_notify(owner);
}

static void input(void)
{
    struct input_event_ring *ring = (void *)input_handoff;
    uint32_t head = __atomic_load_n(&ring->head, __ATOMIC_ACQUIRE);
    uint32_t dropped = __atomic_load_n(&ring->dropped, __ATOMIC_ACQUIRE);
    if (head - driver_tail > INPUT_EVENT_CAPACITY || dropped != driver_dropped) {
        desktop_reset(&router, emit);
        driver_dropped = dropped;
        driver_tail = head;
    }
    unsigned before = router.focus;
    while (driver_tail != head) {
        struct input_event_record event = ring->events[driver_tail++ & INPUT_EVENT_MASK];
#ifdef SURFACE_DYNAMIC
        if (dynamic_control(event)) continue;
#endif
        desktop_route(&router, event, emit);
#ifdef SURFACE_DYNAMIC
        if (!dynamic_active && router.focus == 1) {
            router.focus = 0;
            router.pointer_owner = 0;
        }
#endif
    }
    __atomic_store_n(&ring->tail, driver_tail, __ATOMIC_RELEASE);
    focus_dirty |= before != router.focus;
}
#endif

#define APP_WIDTH 480u
#define APP_HEIGHT 800u
#define APP_BYTES (APP_WIDTH * APP_HEIGHT * 3u)
struct surface_header {
    _Atomic uint32_t revision;
    uint32_t width, height, stride;
};
uintptr_t surface_a, surface_b, output_frame, focus_page;
static unsigned stage;
static uint32_t revisions[2];
static uint8_t composed_pixels[SLINT_FRAME_PIXEL_BYTES];
static uint8_t cached_pixels[2][APP_BYTES];
static uint32_t cached_revisions[2];
static uint8_t candidate_pixels[APP_BYTES];

static uint32_t snapshot(uintptr_t source)
{
    const struct surface_header *header = (const void *)source;
    uint32_t first = atomic_load_explicit(&header->revision, memory_order_acquire);
    if (!first || (first & 1))
        return 0;
    if (header->width != APP_WIDTH || header->height != APP_HEIGHT ||
        header->stride != APP_WIDTH * 3) {
        return 0;
    }
    const volatile uint8_t *pixels = (const volatile uint8_t *)(header + 1);
    for (size_t i = 0; i < APP_BYTES; ++i)
        candidate_pixels[i] = pixels[i];
    atomic_thread_fence(memory_order_acquire);
    if (atomic_load_explicit(&header->revision, memory_order_acquire) != first)
        return 0;
    return first;
}

static bool blit(uintptr_t source, unsigned index, unsigned x, unsigned y, uint32_t *revision)
{
    uint32_t offered = snapshot(source);
    if (offered && offered != cached_revisions[index]) {
        for (size_t i = 0; i < APP_BYTES; ++i)
            cached_pixels[index][i] = candidate_pixels[i];
        cached_revisions[index] = offered;
    }
    if (!cached_revisions[index])
        return false;
    uint8_t *destination = composed_pixels;
    const uint8_t *pixels = cached_pixels[index];
    for (unsigned row = 0; row < APP_HEIGHT; ++row)
        for (unsigned column = 0; column < APP_WIDTH * 3; ++column)
            destination[((y + row) * SLINT_FRAME_WIDTH + x) * 3 + column] =
                pixels[row * APP_WIDTH * 3 + column];
    *revision = cached_revisions[index];
    return true;
}

static bool compose(void)
{
    slint_frame_header_t *output = (void *)output_frame;
    uint32_t next_a, next_b;
    bool ready;
#ifdef SURFACE_DYNAMIC
    next_a = 0;
    if (!dynamic_active || !blit(surface_a, 0, 40, 500, &next_a)) {
        for (unsigned y = 500; y < 1300; ++y)
            for (unsigned x = 40; x < 520; ++x) {
                size_t offset = (y * SLINT_FRAME_WIDTH + x) * 3;
                composed_pixels[offset] = 4;
                composed_pixels[offset + 1] = 12;
                composed_pixels[offset + 2] = 22;
            }
    }
    ready = blit(surface_b, 1, 560, 500, &next_b);
#else
    ready = blit(surface_a, 0, 40, 500, &next_a) && blit(surface_b, 1, 560, 500, &next_b);
#endif
    if (!ready)
        return false;
    if (next_a == revisions[0] && next_b == revisions[1]
#ifdef SURFACE_INTERACTIVE
        && !focus_dirty
#endif
        )
        return true;
#ifdef SURFACE_INTERACTIVE
    /* A trusted border communicates focus outside either app's writable area. */
    for (unsigned index = 1; index <= 2; ++index) {
        unsigned left = index == 1 ? 36 : 556;
        for (unsigned y = 496; y < 1304; ++y)
            for (unsigned x = left; x < left + 488; ++x) {
                if (x >= left + 4 && x < left + 484 && y >= 500 && y < 1300) continue;
                size_t offset = (y * SLINT_FRAME_WIDTH + x) * 3;
                composed_pixels[offset] = router.focus == index ? 104 : 35;
                composed_pixels[offset + 1] = router.focus == index ? 221 : 53;
                composed_pixels[offset + 2] = router.focus == index ? 194 : 69;
            }
    }
    focus_dirty = false;
#endif
    uint32_t revision = atomic_load_explicit(&output->revision, memory_order_relaxed);
    atomic_store_explicit(&output->revision, revision + 1, memory_order_release);
    atomic_thread_fence(memory_order_seq_cst);
    uint8_t *destination = (void *)(output_frame + sizeof(*output));
    for (size_t i = 0; i < SLINT_FRAME_PIXEL_BYTES; ++i)
        destination[i] = composed_pixels[i];
    output->width = SLINT_FRAME_WIDTH;
    output->height = SLINT_FRAME_HEIGHT;
    output->stride_bytes = SLINT_FRAME_STRIDE_BYTES;
    output->format = SLINT_FRAME_FORMAT_RGB8;
    atomic_store_explicit(&output->damage_y, 0, memory_order_relaxed);
    atomic_store_explicit(&output->damage_height, SLINT_FRAME_HEIGHT, memory_order_relaxed);
    atomic_store_explicit(&output->revision, revision + 2, memory_order_release);
    revisions[0] = next_a;
    revisions[1] = next_b;
    microkit_notify(2);
    return true;
}

#ifdef SURFACE_DYNAMIC
/* Caller must suspend the producer before this, and revoke mappings afterward. */
static void discard_dynamic_surface(void)
{
    dynamic_active = false;
    for (unsigned i = 0; i <= DESKTOP_KEY_MAX; ++i) if (router.keys[i] == 1) router.keys[i] = 0;
    if (router.pointer_owner == 1) router.pointer_owner = 0;
    if (router.focus == 1) router.focus = 0;
    sent[0] = 0;
    reset_pending[0] = false;
    cached_revisions[0] = 0;
    revisions[0] = UINT32_MAX;
    for (size_t i = 0; i < sizeof(cached_pixels[0]); ++i) cached_pixels[0][i] = 0;
    focus_dirty = true;
    compose();
    for (size_t i = 0; i < sizeof(candidate_pixels); ++i) candidate_pixels[i] = 0;
}
#endif

void init(void)
{
    *(volatile uint32_t *)focus_page = 1;
    uint8_t *pixels = composed_pixels;
    for (size_t i = 0; i < SLINT_FRAME_PIXEL_BYTES; i += 3) {
        pixels[i] = 4;
        pixels[i + 1] = 12;
        pixels[i + 2] = 22;
    }
}

void notified(microkit_channel channel)
{
#ifdef SURFACE_INTERACTIVE
    if (channel <= 1) flush_reset(channel + 1);
    if (channel == 6) input();
    if (channel > 1 && channel != 6) return;
    if (compose() && !input_ready) {
        input_ready = true;
        microkit_dbg_puts("SURFACE|PASS|interactive ready\n");
    }
    return;
#endif
    if (channel > 1)
        return;
    uint32_t old_a = revisions[0], old_b = revisions[1];
    if (!compose())
        return;
    if (revisions[0] == old_a && revisions[1] == old_b)
        return;
    if (stage == 0) {
        if (revisions[0] != 2 || revisions[1] != 2) {
            microkit_dbg_puts("SURFACE|FAIL|initial revisions\n");
            return;
        }
        microkit_dbg_puts("SURFACE|PASS|two independent Slint surfaces\n");
        stage = 1;
        microkit_notify(0);
    } else if (stage == 1) {
        if (revisions[0] != old_a + 2 || revisions[1] != old_b) {
            microkit_dbg_puts("SURFACE|FAIL|first app update crossed boundary\n");
            return;
        }
        stage = 2;
        microkit_notify(1);
    } else if (stage == 2) {
        if (revisions[1] != old_b + 2 || revisions[0] != old_a) {
            microkit_dbg_puts("SURFACE|FAIL|second app update crossed boundary\n");
            return;
        }
        microkit_dbg_puts("SURFACE|PASS|independent updates\n");
        stage = 3;
        microkit_notify(3);
    } else if (stage == 4) {
        if (revisions[1] != old_b + 2 || revisions[0] != old_a) {
            microkit_dbg_puts("SURFACE|FAIL|surviving app update\n");
            return;
        }
        stage = 5;
        microkit_dbg_puts("SURFACE|PASS|peer fault contained and survivor redrew\n");
        microkit_dbg_puts("SURFACE|PASS|complete\n");
    }
}

seL4_Bool fault(microkit_child child, microkit_msginfo info, microkit_msginfo *reply)
{
    (void)reply;
    if (child != 1 || stage != 3 || microkit_msginfo_get_label(info) != seL4_Fault_VMFault ||
        microkit_mr_get(seL4_VMFault_Addr) != 0x61000000 ||
        microkit_mr_get(seL4_VMFault_PrefetchFault)) {
        microkit_dbg_puts("SURFACE|FAIL|unexpected app fault\n");
    } else {
        microkit_pd_stop(1);
        microkit_dbg_puts("SURFACE|PASS|peer surface memory denied\n");
        stage = 4;
        microkit_notify(1);
    }
    return seL4_False;
}
