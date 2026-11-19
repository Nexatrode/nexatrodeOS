#ifndef NEXATRODE_TOUCH_BOOT_STREAM_H
#define NEXATRODE_TOUCH_BOOT_STREAM_H

#include <stddef.h>
#include <stdint.h>

enum {
    TOUCH_BOOT_WRITE = 1,
    TOUCH_BOOT_WRITE_ACK = 2,
    TOUCH_BOOT_WAIT_IRQ = 3,
    TOUCH_BOOT_SET_TYPE = 4,
};

typedef struct {
    const uint8_t *bytes;
    size_t length;
    size_t offset;
    uint32_t count;
    uint32_t index;
} touch_boot_stream;

typedef struct {
    uint32_t type;
    const uint8_t *data;
    uint32_t length;
} touch_boot_operation;

/* Validates the entire stream before any operation may be read. */
int touch_boot_stream_open(touch_boot_stream *stream, const void *bytes, size_t length);
/* Returns 1 for an operation, 0 at end. Only call after successful open. */
int touch_boot_stream_next(touch_boot_stream *stream, touch_boot_operation *operation);

#endif
