/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 */
#pragma once

#include <stdint.h>

#define INPUT_EVENT_CAPACITY 256U
#define INPUT_EVENT_MASK (INPUT_EVENT_CAPACITY - 1U)
#define INPUT_DEVICE_KEYBOARD 0U
#define INPUT_DEVICE_TABLET 1U
#define INPUT_SERVICE_CHANNEL 2U

struct input_event_record {
    uint16_t device;
    uint16_t type;
    uint16_t code;
    uint16_t reserved;
    uint32_t value;
};

struct input_event_ring {
    volatile uint32_t head;
    volatile uint32_t tail;
    volatile uint32_t dropped;
    uint32_t reserved;
    struct input_event_record events[INPUT_EVENT_CAPACITY];
};

#define INPUT_EV_SYN 0U
#define INPUT_EV_ABS 3U

static inline int input_ring_try_push(struct input_event_ring *ring,
                                      struct input_event_record event)
{
    uint32_t head = __atomic_load_n(&ring->head, __ATOMIC_RELAXED);
    uint32_t tail = __atomic_load_n(&ring->tail, __ATOMIC_ACQUIRE);
    if ((uint32_t)(head - tail) >= INPUT_EVENT_CAPACITY) {
        /* Prefer keeping KEY/BTN: replace the newest pending ABS/SYN slot. */
        if (event.type != INPUT_EV_ABS && event.type != INPUT_EV_SYN) {
            for (uint32_t i = head; i != tail; i--) {
                uint32_t slot = (i - 1U) & INPUT_EVENT_MASK;
                uint16_t type = ring->events[slot].type;
                if (type == INPUT_EV_ABS || type == INPUT_EV_SYN) {
                    ring->events[slot] = event;
                    __atomic_add_fetch(&ring->dropped, 1, __ATOMIC_RELAXED);
                    return 1;
                }
            }
        }
        __atomic_add_fetch(&ring->dropped, 1, __ATOMIC_RELAXED);
        return 0;
    }

    ring->events[head & INPUT_EVENT_MASK] = event;
    __atomic_store_n(&ring->head, head + 1, __ATOMIC_RELEASE);
    return 1;
}

_Static_assert(sizeof(struct input_event_record) == 12,
               "input event record layout must match Rust");
_Static_assert((INPUT_EVENT_CAPACITY & INPUT_EVENT_MASK) == 0,
               "input event ring capacity must be a power of two");
