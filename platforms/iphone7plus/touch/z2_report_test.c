#include "z2_report.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    unsigned calls, selects, delays;
    int selected;
    int status;
    int length;
} fake_z2;

static int fake_select(void *context, int asserted)
{
    fake_z2 *fake = context;
    assert(fake->selected != asserted);
    fake->selected = asserted;
    fake->selects++;
    return 0;
}

static void fake_delay(void *context, unsigned microseconds)
{
    fake_z2 *fake = context;
    assert(microseconds == 1000);
    fake->delays++;
}

static int fake_transfer(void *context, const uint8_t *tx, uint8_t *rx,
                         size_t length)
{
    fake_z2 *fake = context;
    unsigned sum = 0;
    assert(fake->selected);
    if (fake->calls == 0) assert(length == 16 && tx[0] == 0xee);
    if (fake->calls == 1 || fake->calls == 2)
        assert(length == 16 && tx[0] == 0xe3 && tx[1] == 0xd9);
    if (fake->calls == 3)
        assert(length == 16 && tx[0] == 0xe7 && tx[1] == 0xd9 && tx[3] == 16);
    if (fake->calls == 4) {
        assert(length == 21);
        for (unsigned i = 0; i < length; i++) assert(tx[i] == 0xa5);
    } else {
        for (unsigned i = 0; i < 14; i++) sum += tx[i];
        assert(tx[14] == (uint8_t)sum);
        assert(tx[15] == (uint8_t)(sum >> 8));
    }
    memset(rx, 0, length);
    if (fake->calls == 0) rx[0] = 0x11;
    if (fake->calls == 1) rx[0] = 0x22;
    if (fake->calls == 2) {
        rx[0] = 0xa1;
        rx[1] = 0xb2;
        rx[2] = (uint8_t)fake->status;
        rx[3] = (uint8_t)fake->length;
        rx[4] = (uint8_t)(fake->length >> 8);
    }
    if (fake->calls == 4)
        for (unsigned i = 0; i < 16; i++) rx[i + 3] = (uint8_t)(i + 1);
    fake->calls++;
    return 0;
}

int main(void)
{
    fake_z2 fake = { .length = 16 };
    touch_z2_link link = { &fake, fake_select, fake_transfer, fake_delay };
    touch_z2_result result;
    assert(touch_z2_read_d9(&link, &result) == TOUCH_Z2_OK);
    assert(result.stage == 5 && result.controller_length == 16);
    assert(result.info_response[0] == 0xa1 &&
           result.info_response[1] == 0xb2);
    assert(result.wake_response[0] == 0x11 &&
           result.first_info_response[0] == 0x22);
    assert(result.d9[0] == 1 && result.d9[15] == 16);
    assert(fake.calls == 5 && fake.selects == 10 && fake.delays == 10);

    memset(&fake, 0, sizeof(fake));
    fake.length = 16;
    fake.status = 3;
    assert(touch_z2_read_d9(&link, &result) == TOUCH_Z2_STATUS);
    assert(result.stage == 3 && result.controller_status == 3);
    assert(result.info_response[0] == 0xa1 &&
           result.info_response[2] == 3);
    assert(fake.calls == 3 && !fake.selected);

    memset(&fake, 0, sizeof(fake));
    fake.length = 8;
    assert(touch_z2_read_d9(&link, &result) == TOUCH_Z2_LENGTH);
    assert(result.stage == 3 && result.controller_length == 8);
    assert(fake.calls == 3 && !fake.selected);

    puts("D11 touch generation-2 D9 control read: PASS");
    return 0;
}
