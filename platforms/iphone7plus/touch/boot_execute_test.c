#include "boot_execute.h"
#include "boot_stream.h"
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

enum { PAYLOAD_SIZE = 16390, STREAM_SIZE = 16 + 12 + 8 + PAYLOAD_SIZE + 8 };

typedef struct {
    unsigned cs_count, tx_count, delays, irq_count, progress_count;
    unsigned lengths[3], cs[4];
    int fail_transfer;
} fake_link;

static void put32(uint8_t *bytes, uint32_t value)
{
    for (unsigned i = 0; i < 4; i++)
        bytes[i] = value >> (i * 8);
}

static int chip_select(void *context, int asserted)
{
    fake_link *link = context;
    assert(link->cs_count < 4);
    link->cs[link->cs_count++] = asserted;
    return 0;
}

static int transfer(void *context, const uint8_t *bytes, size_t length)
{
    fake_link *link = context;
    assert(link->tx_count < 3);
    link->lengths[link->tx_count++] = (unsigned)length;
    if (length == 2)
        assert(bytes[0] == 0x1a && bytes[1] == 0xa1);
    return link->fail_transfer && link->tx_count == 2;
}

static void delay_us(void *context, unsigned microseconds)
{
    fake_link *link = context;
    assert(microseconds == 1000);
    link->delays++;
}

static int wait_irq(void *context, unsigned timeout_ms)
{
    fake_link *link = context;
    assert(timeout_ms == 500);
    link->irq_count++;
    return 0;
}

static void progress(void *context, uint32_t index, uint32_t type)
{
    fake_link *link = context;
    assert(index == link->progress_count);
    assert(type == (index == 0 ? TOUCH_BOOT_SET_TYPE :
                    index == 1 ? TOUCH_BOOT_WRITE_ACK : TOUCH_BOOT_WAIT_IRQ));
    link->progress_count++;
}

int main(void)
{
    static uint8_t stream[STREAM_SIZE];
    fake_link fake = {0};
    touch_boot_link link = {
        &fake, chip_select, transfer, delay_us, wait_irq, progress
    };
    memcpy(stream, "NTFW", 4);
    put32(stream + 4, 1);
    put32(stream + 8, 3);
    put32(stream + 12, STREAM_SIZE);
    put32(stream + 16, TOUCH_BOOT_SET_TYPE);
    put32(stream + 20, 4);
    put32(stream + 24, 2);
    put32(stream + 28, TOUCH_BOOT_WRITE_ACK);
    put32(stream + 32, PAYLOAD_SIZE);
    put32(stream + STREAM_SIZE - 8, TOUCH_BOOT_WAIT_IRQ);

    assert(touch_boot_execute(stream, sizeof(stream), &link) == TOUCH_BOOT_EXEC_OK);
    assert(fake.cs_count == 4 && fake.cs[0] == 1 && fake.cs[1] == 0 &&
           fake.cs[2] == 1 && fake.cs[3] == 0);
    assert(fake.tx_count == 3 && fake.lengths[0] == 16384 &&
           fake.lengths[1] == 6 && fake.lengths[2] == 2);
    assert(fake.delays == 3 && fake.irq_count == 1 && fake.progress_count == 3);

    memset(&fake, 0, sizeof(fake));
    fake.fail_transfer = 1;
    assert(touch_boot_execute(stream, sizeof(stream), &link) == TOUCH_BOOT_EXEC_IO);
    assert(fake.cs_count == 2 && fake.cs[1] == 0 && fake.irq_count == 0);
    memset(&fake, 0, sizeof(fake));
    stream[0] = 'X';
    assert(touch_boot_execute(stream, sizeof(stream), &link) == TOUCH_BOOT_EXEC_INVALID);
    assert(fake.cs_count == 0 && fake.tx_count == 0);
    puts("D11 touch boot executor: PASS");
    return 0;
}
