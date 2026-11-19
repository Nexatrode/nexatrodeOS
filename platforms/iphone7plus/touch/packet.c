#include "packet.h"

int iphone_touch_parse_packet(const uint8_t *header, size_t header_size,
                              const uint8_t *data, size_t data_size,
                              uint8_t expected_tag, IphoneTouchPacket *packet)
{
    if (!header || !data || !packet || header_size < 5 ||
        (header[0] & 0xfeu) != 0xeau || header[1] != expected_tag ||
        (expected_tag != 1 && expected_tag != 2)) return -1;

    unsigned int header_sum = 0;
    for (size_t i = 0; i < 5; i++) header_sum += header[i];
    if ((header_sum & 0xffu) != 0) return -1;

    size_t wire_size = (size_t)header[2] | ((size_t)header[3] << 8);
    if (wire_size < 2 || wire_size > IPHONE_TOUCH_PACKET_MAX_BYTES ||
        data_size < wire_size) return -1;

    size_t payload_size = wire_size - 2;
    uint16_t transmitted_sum = (uint16_t)data[payload_size] |
                               ((uint16_t)data[payload_size + 1] << 8);
    uint16_t calculated_sum = 0;
    for (size_t i = 0; i < payload_size; i++)
        calculated_sum = (uint16_t)(calculated_sum + data[i]);
    if (calculated_sum != transmitted_sum) return -1;

    packet->payload = data;
    packet->payload_size = payload_size;
    return 0;
}
