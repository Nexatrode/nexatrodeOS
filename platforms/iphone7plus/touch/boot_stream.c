#include "boot_stream.h"

enum { HEADER_SIZE = 16, RECORD_HEADER_SIZE = 8, MAX_RECORDS = 64,
       MAX_RECORD_DATA = 1024 * 1024 };

static uint32_t read32(const uint8_t *bytes)
{
    return (uint32_t)bytes[0] | ((uint32_t)bytes[1] << 8) |
           ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 24);
}

int touch_boot_stream_open(touch_boot_stream *stream, const void *input, size_t length)
{
    const uint8_t *bytes = input;
    uint32_t count;
    size_t offset = HEADER_SIZE;

    if (!stream || !bytes || length < HEADER_SIZE ||
        bytes[0] != 'N' || bytes[1] != 'T' || bytes[2] != 'F' || bytes[3] != 'W' ||
        read32(bytes + 4) != 1 || read32(bytes + 12) != length)
        return 0;
    count = read32(bytes + 8);
    if (count < 3 || count > MAX_RECORDS)
        return 0;

    for (uint32_t index = 0; index < count; index++) {
        uint32_t type, size;
        if (length - offset < RECORD_HEADER_SIZE)
            return 0;
        type = read32(bytes + offset);
        size = read32(bytes + offset + 4);
        offset += RECORD_HEADER_SIZE;
        if (size > MAX_RECORD_DATA || size > length - offset)
            return 0;
        if (index == 0) {
            if (type != TOUCH_BOOT_SET_TYPE || size != 4 ||
                read32(bytes + offset) != 2)
                return 0;
        } else if (index == count - 1) {
            if (type != TOUCH_BOOT_WAIT_IRQ || size != 0)
                return 0;
        } else if ((type != TOUCH_BOOT_WRITE && type != TOUCH_BOOT_WRITE_ACK) ||
                   size == 0) {
            return 0;
        }
        offset += size;
    }
    if (offset != length)
        return 0;

    stream->bytes = bytes;
    stream->length = length;
    stream->offset = HEADER_SIZE;
    stream->count = count;
    stream->index = 0;
    return 1;
}

int touch_boot_stream_next(touch_boot_stream *stream, touch_boot_operation *operation)
{
    size_t offset;
    if (!stream || !operation || stream->index >= stream->count)
        return 0;
    offset = stream->offset;
    operation->type = read32(stream->bytes + offset);
    operation->length = read32(stream->bytes + offset + 4);
    operation->data = stream->bytes + offset + RECORD_HEADER_SIZE;
    stream->offset = offset + RECORD_HEADER_SIZE + operation->length;
    stream->index++;
    return 1;
}
