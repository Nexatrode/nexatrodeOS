#include "packet.h"
#include "report.h"

#include <assert.h>
#include <stdint.h>

static void make_header(uint8_t header[5], uint8_t tag, size_t size)
{
    header[0] = 0xea;
    header[1] = tag;
    header[2] = (uint8_t)size;
    header[3] = (uint8_t)(size >> 8);
    header[4] = (uint8_t)(0u - header[0] - header[1] - header[2] - header[3]);
}

int main(void)
{
    uint8_t header[5];
    uint8_t data[326] = {0};
    IphoneTouchPacket packet = {0};
    make_header(header, 1, 6);
    data[0] = 1; data[1] = 2; data[2] = 3; data[3] = 4;
    data[4] = 10;
    assert(iphone_touch_parse_packet(header, 5, data, 6, 1, &packet) == 0);
    assert(packet.payload == data && packet.payload_size == 4);

    packet.payload = NULL;
    assert(iphone_touch_parse_packet(header, 4, data, 6, 1, &packet) != 0);
    assert(iphone_touch_parse_packet(header, 5, data, 5, 1, &packet) != 0);
    assert(iphone_touch_parse_packet(header, 5, data, 6, 2, &packet) != 0);
    assert(packet.payload == NULL);

    header[4]++;
    assert(iphone_touch_parse_packet(header, 5, data, 6, 1, &packet) != 0);
    make_header(header, 1, 6);
    data[0]++;
    assert(iphone_touch_parse_packet(header, 5, data, 6, 1, &packet) != 0);
    data[0]--;

    make_header(header, 1, 327);
    assert(iphone_touch_parse_packet(header, 5, data, 326, 1, &packet) != 0);
    make_header(header, 1, 1);
    assert(iphone_touch_parse_packet(header, 5, data, 326, 1, &packet) != 0);

    make_header(header, 2, 326);
    for (size_t i = 0; i < 324; i++) data[i] = 0xff;
    uint16_t sum = (uint16_t)(324u * 0xffu);
    data[324] = (uint8_t)sum;
    data[325] = (uint8_t)(sum >> 8);
    assert(iphone_touch_parse_packet(header, 5, data, 326, 2, &packet) == 0);
    assert(packet.payload_size == 324);

    for (size_t i = 0; i < sizeof(data); i++) data[i] = 0;
    make_header(header, 1, 56);
    data[16] = 1;
    data[24] = 3;
    data[25] = 4;
    data[28] = 0xe8; data[29] = 0x03;
    data[30] = 0xd0; data[31] = 0x07;
    sum = 0;
    for (size_t i = 0; i < 54; i++) sum = (uint16_t)(sum + data[i]);
    data[54] = (uint8_t)sum;
    data[55] = (uint8_t)(sum >> 8);
    assert(iphone_touch_parse_packet(header, 5, data, 56, 1, &packet) == 0);
    IphoneTouchCalibration calibration = {0, 4000, 0, 4000};
    IphoneTouchFrame frame = {0};
    assert(iphone_touch_decode_report(packet.payload, packet.payload_size,
                                      &calibration, &frame) == 0);
    assert(frame.count == 1 && frame.contacts[0].finger == 3);
    assert(frame.contacts[0].active == 1);
    assert(frame.contacts[0].x == 1024 && frame.contacts[0].y == 2048);
    return 0;
}
