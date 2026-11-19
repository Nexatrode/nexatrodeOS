#ifndef NEXATRODE_TOUCH_BOOT_EXECUTE_H
#define NEXATRODE_TOUCH_BOOT_EXECUTE_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    void *context;
    int (*chip_select)(void *context, int asserted);
    int (*transfer)(void *context, const uint8_t *bytes, size_t length);
    void (*delay_us)(void *context, unsigned microseconds);
    int (*wait_irq)(void *context, unsigned timeout_ms);
    void (*progress)(void *context, uint32_t index, uint32_t type);
} touch_boot_link;

enum {
    TOUCH_BOOT_EXEC_OK = 0,
    TOUCH_BOOT_EXEC_INVALID = -1,
    TOUCH_BOOT_EXEC_IO = -2,
    TOUCH_BOOT_EXEC_IRQ = -3
};

/* Validate the complete private stream before sending any SPI bytes. Each
 * record holds CS across its 16 KiB chunks, then deasserts it on all paths. */
int touch_boot_execute(const void *stream_bytes, size_t stream_length,
                       const touch_boot_link *link);

#endif
