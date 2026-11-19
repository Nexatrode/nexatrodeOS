#pragma once

#include <stddef.h>
#include <stdint.h>

enum { IPHONE_TOUCH_MAX_CONTACTS = 10, IPHONE_TOUCH_AXIS_MAX = 4096 };

typedef struct {
    int32_t left;
    int32_t right;
    int32_t top;
    int32_t bottom;
} IphoneTouchCalibration;

typedef struct {
    uint8_t finger;
    uint8_t active;
    uint16_t x;
    uint16_t y;
} IphoneTouchContact;

typedef struct {
    uint8_t count;
    IphoneTouchContact contacts[IPHONE_TOUCH_MAX_CONTACTS];
} IphoneTouchFrame;

/* Decode the checksum-verified payload returned by the touch transport.
 * This function never reads MMIO, configures SPI, or loads firmware. */
int iphone_touch_decode_report(const uint8_t *payload, size_t payload_size,
                               const IphoneTouchCalibration *calibration,
                               IphoneTouchFrame *frame);
