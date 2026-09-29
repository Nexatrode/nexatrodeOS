/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <assert.h>
#include <stdint.h>
#include <string.h>

#include "input_handoff.h"

int main(void)
{
    struct input_event_ring ring;
    memset(&ring, 0, sizeof(ring));

    for (uint32_t i = 0; i < INPUT_EVENT_CAPACITY; i++) {
        struct input_event_record event = {
            .device = INPUT_DEVICE_KEYBOARD,
            .type = 1,
            .code = (uint16_t)(i + 1),
            .reserved = 0,
            .value = i & 1U,
        };
        assert(input_ring_try_push(&ring, event));
    }
    assert(!input_ring_try_push(&ring, (struct input_event_record){0}));
    assert(__atomic_load_n(&ring.head, __ATOMIC_ACQUIRE) == INPUT_EVENT_CAPACITY);
    assert(__atomic_load_n(&ring.dropped, __ATOMIC_ACQUIRE) == 1);

    __atomic_store_n(&ring.tail, 16, __ATOMIC_RELEASE);
    for (uint32_t i = 0; i < 16; i++) {
        struct input_event_record event = {
            .device = INPUT_DEVICE_TABLET,
            .type = 3,
            .code = 0,
            .reserved = 0,
            .value = i,
        };
        assert(input_ring_try_push(&ring, event));
    }
    assert(!input_ring_try_push(&ring, (struct input_event_record){0}));
    assert(__atomic_load_n(&ring.head, __ATOMIC_ACQUIRE) == 48);
    assert(__atomic_load_n(&ring.dropped, __ATOMIC_ACQUIRE) == 2);
    assert(ring.events[15].code == 0);
    assert(ring.events[15].value == 15);

    /* Monotonic counters wrap safely because occupancy stays below capacity. */
    memset(&ring, 0, sizeof(ring));
    __atomic_store_n(&ring.head, UINT32_MAX - 1, __ATOMIC_RELAXED);
    __atomic_store_n(&ring.tail, UINT32_MAX - 1, __ATOMIC_RELAXED);
    assert(input_ring_try_push(&ring, (struct input_event_record){.value = 11}));
    assert(input_ring_try_push(&ring, (struct input_event_record){.value = 12}));
    assert(__atomic_load_n(&ring.head, __ATOMIC_ACQUIRE) == 0);
    assert((uint32_t)(__atomic_load_n(&ring.head, __ATOMIC_ACQUIRE) -
                      __atomic_load_n(&ring.tail, __ATOMIC_ACQUIRE)) == 2);
    assert(ring.events[30].value == 11);
    assert(ring.events[31].value == 12);
    return 0;
}
