#include "boot_execute.h"
#include "boot_stream.h"

enum { CHUNK_SIZE = 16384 };

static int send(const touch_boot_link *link, const uint8_t *bytes, size_t length)
{
    size_t offset = 0;
    int result = TOUCH_BOOT_EXEC_OK;
    if (link->chip_select(link->context, 1))
        return TOUCH_BOOT_EXEC_IO;
    link->delay_us(link->context, 1000);
    while (offset < length) {
        size_t chunk = length - offset;
        if (chunk > CHUNK_SIZE)
            chunk = CHUNK_SIZE;
        if (link->transfer(link->context, bytes + offset, chunk)) {
            result = TOUCH_BOOT_EXEC_IO;
            break;
        }
        offset += chunk;
    }
    if (link->chip_select(link->context, 0))
        result = TOUCH_BOOT_EXEC_IO;
    return result;
}

int touch_boot_execute(const void *stream_bytes, size_t stream_length,
                       const touch_boot_link *link)
{
    static const uint8_t ack[2] = { 0x1a, 0xa1 };
    touch_boot_stream stream;
    touch_boot_operation operation;
    if (!link || !link->chip_select || !link->transfer ||
        !link->delay_us || !link->wait_irq ||
        !touch_boot_stream_open(&stream, stream_bytes, stream_length))
        return TOUCH_BOOT_EXEC_INVALID;
    while (touch_boot_stream_next(&stream, &operation)) {
        if (link->progress)
            link->progress(link->context, stream.index - 1, operation.type);
        switch (operation.type) {
        case TOUCH_BOOT_SET_TYPE:
            break; /* The stream validator requires generation 2. */
        case TOUCH_BOOT_WRITE:
        case TOUCH_BOOT_WRITE_ACK:
            if (send(link, operation.data, operation.length))
                return TOUCH_BOOT_EXEC_IO;
            if (operation.type == TOUCH_BOOT_WRITE_ACK) {
                if (send(link, ack, sizeof(ack)))
                    return TOUCH_BOOT_EXEC_IO;
                link->delay_us(link->context, 1000);
            }
            break;
        case TOUCH_BOOT_WAIT_IRQ:
            if (link->wait_irq(link->context, 500))
                return TOUCH_BOOT_EXEC_IRQ;
            break;
        default:
            return TOUCH_BOOT_EXEC_INVALID;
        }
    }
    return TOUCH_BOOT_EXEC_OK;
}
