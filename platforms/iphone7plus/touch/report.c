#include "report.h"

enum { HEADER_BYTES = 24, CONTACT_BYTES = 30, COUNT_OFFSET = 16 };

static int16_t read_signed_le16(const uint8_t *bytes)
{
    int32_t value = (int32_t)bytes[0] | ((int32_t)bytes[1] << 8);
    if (value >= 0x8000) value -= 0x10000;
    return (int16_t)value;
}

static uint16_t normalize(int32_t raw, int32_t low, int32_t high)
{
    int64_t coordinate = ((int64_t)raw - low) * IPHONE_TOUCH_AXIS_MAX /
                         ((int64_t)high - low);
    if (coordinate < 0) return 0;
    if (coordinate > IPHONE_TOUCH_AXIS_MAX) return IPHONE_TOUCH_AXIS_MAX;
    return (uint16_t)coordinate;
}

int iphone_touch_decode_report(const uint8_t *payload, size_t payload_size,
                               const IphoneTouchCalibration *calibration,
                               IphoneTouchFrame *frame)
{
    if (!payload || !calibration || !frame || payload_size < HEADER_BYTES ||
        calibration->left == calibration->right ||
        calibration->top == calibration->bottom) {
        return -1;
    }

    uint8_t count = payload[COUNT_OFFSET];
    if (count > IPHONE_TOUCH_MAX_CONTACTS ||
        payload_size < HEADER_BYTES + (size_t)count * CONTACT_BYTES) {
        return -1;
    }

    IphoneTouchFrame decoded = { .count = count };
    for (size_t index = 0; index < count; index++) {
        const uint8_t *contact = payload + HEADER_BYTES + index * CONTACT_BYTES;
        decoded.contacts[index].finger = contact[0];
        decoded.contacts[index].active = contact[1] == 4;
        decoded.contacts[index].x = normalize(read_signed_le16(contact + 4),
                                              calibration->left,
                                              calibration->right);
        decoded.contacts[index].y = normalize(read_signed_le16(contact + 6),
                                              calibration->top,
                                              calibration->bottom);
    }
    *frame = decoded;
    return 0;
}
