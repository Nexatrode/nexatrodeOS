#include "boot_stream.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>

static void put32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = value;
    bytes[1] = value >> 8;
    bytes[2] = value >> 16;
    bytes[3] = value >> 24;
}

int main(void)
{
    uint8_t data[48] = { 'N', 'T', 'F', 'W' };
    touch_boot_stream stream;
    touch_boot_operation operation;
    put32(data + 4, 1);
    put32(data + 8, 3);
    put32(data + 12, sizeof(data));
    put32(data + 16, TOUCH_BOOT_SET_TYPE);
    put32(data + 20, 4);
    put32(data + 24, 2);
    put32(data + 28, TOUCH_BOOT_WRITE);
    put32(data + 32, 4);
    data[36] = 0x1a;
    data[37] = 0xa1;
    data[38] = 0x18;
    data[39] = 0xe1;
    put32(data + 40, TOUCH_BOOT_WAIT_IRQ);
    put32(data + 44, 0);

    assert(touch_boot_stream_open(&stream, data, sizeof(data)));
    assert(touch_boot_stream_next(&stream, &operation));
    assert(operation.type == TOUCH_BOOT_SET_TYPE && operation.length == 4);
    assert(touch_boot_stream_next(&stream, &operation));
    assert(operation.type == TOUCH_BOOT_WRITE && operation.length == 4 &&
           operation.data[0] == 0x1a && operation.data[3] == 0xe1);
    assert(touch_boot_stream_next(&stream, &operation));
    assert(operation.type == TOUCH_BOOT_WAIT_IRQ && operation.length == 0);
    assert(!touch_boot_stream_next(&stream, &operation));

    assert(!touch_boot_stream_open(&stream, data, sizeof(data) - 1));
    data[12] = 47;
    assert(!touch_boot_stream_open(&stream, data, sizeof(data)));
    data[12] = 48;
    put32(data + 32, 0xFFFFFFFF);
    assert(!touch_boot_stream_open(&stream, data, sizeof(data)));
    put32(data + 32, 4);
    put32(data + 40, TOUCH_BOOT_WRITE_ACK);
    assert(!touch_boot_stream_open(&stream, data, sizeof(data)));
    put32(data + 40, TOUCH_BOOT_WAIT_IRQ);
    put32(data + 16, TOUCH_BOOT_WRITE);
    assert(!touch_boot_stream_open(&stream, data, sizeof(data)));
    puts("D11 touch boot stream reader: PASS");
    return 0;
}
