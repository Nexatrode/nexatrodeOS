#include "z2_report.h"
#include "packet.h"

static void clear_bytes(uint8_t *bytes, size_t count)
{
    for (size_t i = 0; i < count; i++)
        bytes[i] = 0;
}

static int send_command(const touch_z2_link *link, uint8_t *command,
                        uint8_t response[16])
{
    uint16_t sum = 0;
    int failed;
    for (unsigned i = 0; i < 14; i++)
        sum = (uint16_t)(sum + command[i]);
    command[14] = (uint8_t)sum;
    command[15] = (uint8_t)(sum >> 8);
    if (link->chip_select(link->context, 1))
        return TOUCH_Z2_IO;
    link->delay_us(link->context, 1000);
    failed = link->transfer(link->context, command, response, 16);
    if (link->chip_select(link->context, 0))
        failed = 1;
    link->delay_us(link->context, 1000);
    return failed ? TOUCH_Z2_IO : TOUCH_Z2_OK;
}

int touch_z2_read_d9(const touch_z2_link *link, touch_z2_result *result)
{
    uint8_t command[16], response[21], filler[21];
    int error;
    if (!link || !result || !link->chip_select || !link->transfer ||
        !link->delay_us)
        return TOUCH_Z2_INVALID;

    result->stage = 1;
    result->controller_status = 0;
    result->controller_length = 0;
    clear_bytes(result->wake_response, sizeof(result->wake_response));
    clear_bytes(result->first_info_response,
                sizeof(result->first_info_response));
    clear_bytes(result->info_response, sizeof(result->info_response));
    clear_bytes(result->d9, sizeof(result->d9));
    clear_bytes(command, sizeof(command));
    command[0] = 0xee; /* generation-2 wake */
    error = send_command(link, command, response);
    if (error) return error;
    for (unsigned i = 0; i < sizeof(result->wake_response); i++)
        result->wake_response[i] = response[i];

    result->stage = 2;
    clear_bytes(command, sizeof(command));
    command[0] = 0xe3; /* report information */
    command[1] = 0xd9;
    error = send_command(link, command, response);
    if (error) return error;
    for (unsigned i = 0; i < sizeof(result->first_info_response); i++)
        result->first_info_response[i] = response[i];

    result->stage = 3;
    error = send_command(link, command, response);
    if (error) return error;
    for (unsigned i = 0; i < sizeof(result->info_response); i++)
        result->info_response[i] = response[i];
    result->controller_status = response[2];
    result->controller_length = (uint32_t)response[3] |
                                ((uint32_t)response[4] << 8);
    if (result->controller_status)
        return TOUCH_Z2_STATUS;
    if (result->controller_length < 16)
        return TOUCH_Z2_LENGTH;

    result->stage = 4;
    clear_bytes(command, sizeof(command));
    command[0] = 0xe7; /* long report read setup */
    command[1] = 0xd9;
    command[3] = 16;
    error = send_command(link, command, response);
    if (error) return error;

    result->stage = 5;
    for (unsigned i = 0; i < sizeof(filler); i++)
        filler[i] = 0xa5;
    if (link->chip_select(link->context, 1))
        return TOUCH_Z2_IO;
    link->delay_us(link->context, 1000);
    error = link->transfer(link->context, filler, response,
                           sizeof(filler));
    if (link->chip_select(link->context, 0))
        error = 1;
    link->delay_us(link->context, 1000);
    if (error)
        return TOUCH_Z2_IO;
    for (unsigned i = 0; i < sizeof(result->d9); i++)
        result->d9[i] = response[i + 3];
    return TOUCH_Z2_OK;
}

int touch_z2_enable_reports(const touch_z2_link *link)
{
    uint8_t command[16], response[16];
    if (!link || !link->chip_select || !link->transfer || !link->delay_us)
        return TOUCH_Z2_INVALID;
    clear_bytes(command, sizeof(command));
    command[0] = 0xe4; /* short write, report AF, one zero byte */
    command[1] = 0xaf;
    command[2] = 1;
    if (send_command(link, command, response)) return TOUCH_Z2_IO;
    clear_bytes(command, sizeof(command));
    command[0] = 0xe1; /* commit the short write */
    return send_command(link, command, response);
}

int touch_z2_read_frame(const touch_z2_link *link, unsigned tag,
                        touch_z2_frame *frame)
{
    uint8_t command[16], response[16], first[64], filler[64];
    IphoneTouchPacket packet;
    size_t length, initial;
    int failed = 0;
    if (!link || !frame || tag > 1 || !link->chip_select ||
        !link->transfer || !link->delay_us)
        return TOUCH_Z2_INVALID;
    clear_bytes(command, sizeof(command));
    command[0] = 0xeb;
    command[1] = (uint8_t)(tag + 1);
    if (send_command(link, command, response)) return TOUCH_Z2_IO;
    for (unsigned i = 0; i < sizeof(filler); i++) filler[i] = 0xa5;
    if (link->chip_select(link->context, 1)) return TOUCH_Z2_IO;
    link->delay_us(link->context, 1000);
    if (link->transfer(link->context, filler, first, sizeof(first))) {
        failed = 1;
        goto finish;
    }
    for (unsigned i = 0; i < sizeof(frame->header); i++)
        frame->header[i] = first[i];
    length = (size_t)first[2] | ((size_t)first[3] << 8);
    frame->wire_length = (uint32_t)length;
    frame->payload_length = 0;
    if (length < 2 || length > sizeof(frame->data)) {
        failed = TOUCH_Z2_PACKET;
        goto finish;
    }
    initial = length < sizeof(first) - 5 ? length : sizeof(first) - 5;
    for (size_t i = 0; i < initial; i++) frame->data[i] = first[5 + i];
    if (length > initial &&
        link->transfer(link->context, NULL, frame->data + initial,
                       length - initial)) {
        failed = 1;
        goto finish;
    }
finish:
    if (link->chip_select(link->context, 0)) failed = 1;
    link->delay_us(link->context, 1000);
    if (failed) return failed == TOUCH_Z2_PACKET ? failed : TOUCH_Z2_IO;
    if (iphone_touch_parse_packet(frame->header, sizeof(frame->header),
                                  frame->data, length, (uint8_t)(tag + 1),
                                  &packet))
        return TOUCH_Z2_PACKET;
    frame->payload_length = (uint32_t)packet.payload_size;
    return TOUCH_Z2_OK;
}
