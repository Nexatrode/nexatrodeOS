#include "report.h"

#include <assert.h>
#include <stdint.h>

static void put_signed_le16(uint8_t *bytes, int16_t value)
{
    bytes[0] = (uint8_t)value;
    bytes[1] = (uint8_t)((uint16_t)value >> 8);
}

int main(void)
{
    IphoneTouchCalibration calibration = { 0, 4000, 0, 4000 };
    IphoneTouchFrame frame = { .count = 99 };
    uint8_t packet[24 + 2 * 30] = { 0 };
    packet[16] = 2;
    packet[24] = 7;
    packet[25] = 4;
    put_signed_le16(packet + 28, 1000);
    put_signed_le16(packet + 30, 2000);
    packet[54] = 9;
    packet[55] = 0;
    put_signed_le16(packet + 58, -100);
    put_signed_le16(packet + 60, 4500);

    assert(iphone_touch_decode_report(packet, sizeof(packet), &calibration, &frame) == 0);
    assert(frame.count == 2);
    assert(frame.contacts[0].finger == 7 && frame.contacts[0].active == 1);
    assert(frame.contacts[0].x == 1024 && frame.contacts[0].y == 2048);
    assert(frame.contacts[1].finger == 9 && frame.contacts[1].active == 0);
    assert(frame.contacts[1].x == 0 && frame.contacts[1].y == 4096);

    frame.count = 99;
    assert(iphone_touch_decode_report(packet, 23, &calibration, &frame) != 0);
    assert(frame.count == 99);
    assert(iphone_touch_decode_report(packet, sizeof(packet) - 1, &calibration, &frame) != 0);
    assert(frame.count == 99);
    packet[16] = IPHONE_TOUCH_MAX_CONTACTS + 1;
    assert(iphone_touch_decode_report(packet, sizeof(packet), &calibration, &frame) != 0);
    assert(frame.count == 99);
    packet[16] = 2;
    calibration.right = 0;
    assert(iphone_touch_decode_report(packet, sizeof(packet), &calibration, &frame) != 0);
    assert(frame.count == 99);

    calibration.left = 4000;
    assert(iphone_touch_decode_report(packet, sizeof(packet), &calibration, &frame) == 0);
    assert(frame.contacts[0].x == 3072);
    return 0;
}
