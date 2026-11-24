/* Exercise the real compositor's bounded snapshots and last-good-frame cache. */
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "../experiments/desktop-surfaces/compositor.c"

struct fixture_surface {
    struct surface_header header;
    uint8_t pixels[APP_BYTES];
};
static struct fixture_surface first, second;
static uint8_t output[SLINT_FRAME_REGION_BYTES];
static uint32_t focus;
#ifdef SURFACE_INTERACTIVE
static struct desktop_queue first_input, second_input;
static struct input_event_ring driver_input;
#endif

static void offer(struct fixture_surface *surface, uint32_t revision, uint8_t colour)
{
    surface->header.width = APP_WIDTH;
    surface->header.height = APP_HEIGHT;
    surface->header.stride = APP_WIDTH * 3;
    memset(surface->pixels, colour, sizeof(surface->pixels));
    atomic_store(&surface->header.revision, revision);
}

static uint8_t visible(unsigned x)
{
    return output[sizeof(slint_frame_header_t) + (500 * SLINT_FRAME_WIDTH + x) * 3];
}

int main(void)
{
    surface_a = (uintptr_t)&first;
    surface_b = (uintptr_t)&second;
    output_frame = (uintptr_t)output;
    focus_page = (uintptr_t)&focus;
#ifdef SURFACE_INTERACTIVE
    input_a = (uintptr_t)&first_input;
    input_b = (uintptr_t)&second_input;
    input_handoff = (uintptr_t)&driver_input;
#endif
    init();
    assert(!compose());
    offer(&first, 2, 21);
    offer(&second, 2, 42);
    assert(compose());
    assert(visible(40) == 21 && visible(560) == 42);
    uint32_t before = ((slint_frame_header_t *)output)->revision;
    assert(compose());
    assert(((slint_frame_header_t *)output)->revision == before);

    /* A producer that stops halfway through a frame cannot stall its peer. */
    offer(&first, 3, 99);
    offer(&second, 4, 43);
    assert(compose());
    assert(visible(40) == 21 && visible(560) == 43);
    assert(revisions[0] == 2 && revisions[1] == 4);

    /* Malformed dimensions retain the prior snapshot, with fixed copy bounds. */
    offer(&first, 4, 100);
    first.header.width = UINT32_MAX;
    offer(&second, 6, 44);
    assert(compose());
    assert(visible(40) == 21 && visible(560) == 44);
    assert(revisions[0] == 2 && revisions[1] == 6);

    offer(&first, 6, 22);
    assert(compose());
    assert(visible(40) == 22 && visible(560) == 44);
    assert(revisions[0] == 6);
#ifdef SURFACE_INTERACTIVE
    for (unsigned i = 0; i < DESKTOP_EVENT_CAPACITY; ++i)
        emit(1, (struct desktop_event){.kind = DESKTOP_KEY, .code = 30, .value = 1});
    assert(sent[0] == 64 && first_input.tail == 0);
    struct desktop_event oldest = first_input.events[0];
    emit(1, (struct desktop_event){.kind = DESKTOP_KEY, .code = 30, .value = 0});
    assert(reset_pending[0] && sent[0] == 64 && first_input.tail == 0);
    assert(!memcmp(&oldest, &first_input.events[0], sizeof(oldest)));
    /* The other app remains usable while the first app is stalled. */
    emit(2, (struct desktop_event){.kind = DESKTOP_KEY, .code = 48, .value = 1});
    assert(sent[1] == 1 && second_input.head == 1);
    first_input.tail = 64;
    flush_reset(1);
    assert(!reset_pending[0] && first_input.head == 65 && first_input.events[0].kind == DESKTOP_RESET);
    first_input.tail = UINT32_MAX; /* An app cannot redirect the producer's copy bounds. */
    emit(1, (struct desktop_event){.kind = DESKTOP_KEY});
    assert(reset_pending[0] && sent[0] == 65 && sent[1] == 1);
    driver_input.head = INPUT_EVENT_CAPACITY + 1;
    input();
    assert(driver_tail == driver_input.head && driver_input.tail == driver_input.head);
    puts("PASS bounded input queues, overflow reset and stalled consumer containment");
#endif
    puts("PASS independent redraw, stalled peer, malformed bounds, and recovery");
}
