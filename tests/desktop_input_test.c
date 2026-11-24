#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "../experiments/desktop-surfaces/input.h"

static struct { unsigned owner; struct desktop_event event; } output[100];
static unsigned count;
static void record(unsigned owner, struct desktop_event event)
{
    assert(count < 100 && owner >= 1 && owner <= 2);
    output[count].owner = owner;
    output[count++].event = event;
}
static void key(struct desktop_router *router, unsigned code, unsigned value)
{
    desktop_route(router, (struct input_event_record){.type = 1, .code = code, .value = value}, record);
}
int main(void)
{
    struct desktop_router router = {0};
    assert(desktop_hit(40, 500) == 1 && desktop_hit(519, 1299) == 1);
    assert(!desktop_hit(520, 500) && !desktop_hit(40, 1300));
    assert(desktop_hit(560, 500) == 2 && !desktop_hit(1040, 500));
    key(&router, 30, 1);
    key(&router, 30, 0);
    assert(count == 0); /* No focus, no delivery. */
    desktop_focus(&router, 1, record);
    key(&router, 30, 1);
    key(&router, 15, 1);
    key(&router, 15, 2);
    key(&router, 15, 0);
    assert(router.focus == 2); /* Tab repeat cannot cycle repeatedly. */
    key(&router, 30, 2);
    key(&router, 30, 0);
    assert(output[count - 1].owner == 1 && output[count - 2].owner == 1);
    key(&router, 48, 1);
    assert(output[count - 1].owner == 2);
    unsigned before = count;
    key(&router, 768, 1);
    key(&router, 65535, 1);
    key(&router, 30, 3);
    assert(count == before);

    router.x = 100; router.y = 700;
    struct input_event_record button = {.device = 1, .type = 1, .code = 272, .value = 1};
    desktop_route(&router, button, record);
    assert(router.focus == 1 && router.pointer_owner == 1);
    router.x = 600; /* Drag across windows retains the original owner. */
    button.value = 0;
    desktop_route(&router, button, record);
    assert(output[count - 1].owner == 1 && router.pointer_owner == 0);
    desktop_reset(&router, record);
    assert(output[count - 1].event.kind == DESKTOP_RESET && !router.keys[48]);
    before = count;
    key(&router, 48, 0); /* An old release after reset has no recipient. */
    assert(count == before);
    puts("PASS focus, key ownership, pointer capture, malformed input and reset");
}
