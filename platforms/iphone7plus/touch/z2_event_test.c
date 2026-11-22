#include "z2_report.h"
#include "report.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct { unsigned call; int selected; int corrupt; } fake_link;

static int select_chip(void *context, int asserted)
{
    fake_link *fake = context;
    assert(fake->selected != asserted);
    fake->selected = asserted;
    return 0;
}

static void delay_us(void *context, unsigned delay)
{
    (void)context;
    assert(delay == 1000);
}

static int transfer(void *context, const uint8_t *tx, uint8_t *rx, size_t length)
{
    fake_link *fake = context;
    unsigned sum = 0;
    assert(fake->selected);
    memset(rx, 0, length);
    if (fake->call < 3) {
        assert(length == 16);
        for (unsigned i = 0; i < 14; i++) sum += tx[i];
        assert(tx[14] == (uint8_t)sum && tx[15] == (uint8_t)(sum >> 8));
        if (fake->call == 0)
            assert(tx[0] == 0xe4 && tx[1] == 0xaf && tx[2] == 1 && tx[3] == 0);
        if (fake->call == 1) assert(tx[0] == 0xe1);
        if (fake->call == 2) assert(tx[0] == 0xeb && tx[1] == 1);
    } else {
        assert(fake->call == 3 && length == 64);
        for (unsigned i = 0; i < length; i++) assert(tx[i] == 0xa5);
        rx[0] = 0xea;
        rx[1] = 1;
        rx[2] = 56;
        rx[4] = (uint8_t)(0u - rx[0] - rx[1] - rx[2]);
        rx[5 + 16] = 1; /* one contact */
        rx[5 + 24] = 3; /* finger */
        rx[5 + 25] = 4; /* active */
        rx[5 + 28] = 0xe8; rx[5 + 29] = 0x03; /* x = 1000 */
        rx[5 + 30] = 0xd0; rx[5 + 31] = 0x07; /* y = 2000 */
        for (unsigned i = 0; i < 54; i++) sum += rx[5 + i];
        rx[5 + 54] = (uint8_t)sum;
        rx[5 + 55] = (uint8_t)(sum >> 8);
        if (fake->corrupt) rx[5 + 54]++;
    }
    fake->call++;
    return 0;
}

int main(void)
{
    fake_link fake = {0};
    touch_z2_link link = { &fake, select_chip, transfer, delay_us };
    touch_z2_frame frame = {0};
    IphoneTouchCalibration calibration = {0, 4000, 0, 4000};
    IphoneTouchFrame contacts = {0};
    assert(touch_z2_enable_reports(&link) == TOUCH_Z2_OK);
    assert(touch_z2_read_frame(&link, 0, &frame) == TOUCH_Z2_OK);
    assert(frame.wire_length == 56 && frame.payload_length == 54);
    assert(iphone_touch_decode_report(frame.data, frame.payload_length,
                                      &calibration, &contacts) == 0);
    assert(contacts.count == 1 && contacts.contacts[0].finger == 3);
    assert(contacts.contacts[0].active == 1);
    assert(contacts.contacts[0].x == 1024 && contacts.contacts[0].y == 2048);
    assert(fake.call == 4 && !fake.selected);

    fake = (fake_link){ .call = 2, .corrupt = 1 };
    assert(touch_z2_read_frame(&link, 0, &frame) == TOUCH_Z2_PACKET);
    assert(!fake.selected);
    puts("D11 touch generation-2 event read: PASS");
    return 0;
}
