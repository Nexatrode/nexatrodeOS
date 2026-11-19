#ifndef NEXATRODE_TOUCH_Z2_REPORT_H
#define NEXATRODE_TOUCH_Z2_REPORT_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    void *context;
    int (*chip_select)(void *context, int asserted);
    int (*transfer)(void *context, const uint8_t *tx, uint8_t *rx,
                    size_t length);
    void (*delay_us)(void *context, unsigned microseconds);
} touch_z2_link;

enum {
    TOUCH_Z2_OK = 0,
    TOUCH_Z2_INVALID = -1,
    TOUCH_Z2_IO = -2,
    TOUCH_Z2_STATUS = -3,
    TOUCH_Z2_LENGTH = -4,
    TOUCH_Z2_PACKET = -5,
    TOUCH_Z2_NO_EVENT = -6
};

typedef struct {
    uint32_t stage;
    uint32_t controller_status;
    uint32_t controller_length;
    uint8_t wake_response[16];
    uint8_t first_info_response[16];
    uint8_t info_response[16];
    uint8_t d9[16];
} touch_z2_result;

/* Wake generation-2 touch firmware, then read the first 16 D9 bytes. */
int touch_z2_read_d9(const touch_z2_link *link, touch_z2_result *result);

typedef struct {
    uint8_t header[5];
    uint8_t data[326];
    uint32_t wire_length;
    uint32_t payload_length;
} touch_z2_frame;

/* Start generation-2 contact reports, then read one bounded IRQ packet. */
int touch_z2_enable_reports(const touch_z2_link *link);
int touch_z2_read_frame(const touch_z2_link *link, unsigned tag,
                        touch_z2_frame *frame);

#endif
