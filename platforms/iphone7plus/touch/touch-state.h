#ifndef NEXATRODE_IPHONE_TOUCH_STATE_H
#define NEXATRODE_IPHONE_TOUCH_STATE_H

#include <stdint.h>

enum {
    IPHONE_TOUCH_STARTING = 1,
    IPHONE_TOUCH_READY = 2,
    IPHONE_TOUCH_ERROR = 3,
    IPHONE_TOUCH_EVENT_CAPACITY = 256
};

typedef struct {
    volatile uint32_t revision;
    volatile uint32_t active;
    volatile uint32_t finger;
    volatile uint32_t x;
    volatile uint32_t y;
} IphoneTouchEvent;

/* A single writer publishes a snapshot by changing revision odd/even.
 * The phone shell reads only complete even revisions. */
typedef struct {
    volatile uint32_t revision;
    volatile uint32_t status;
    volatile uint32_t stage;
    volatile uint32_t packets;
    volatile uint32_t active;
    volatile uint32_t finger;
    volatile uint32_t x;
    volatile uint32_t y;
    /* Bounded single-producer queue. The older snapshot fields stay first. */
    volatile uint32_t event_write;
    IphoneTouchEvent events[IPHONE_TOUCH_EVENT_CAPACITY];
} IphoneTouchState;

#endif
