#pragma once

#include <stddef.h>
#include <stdint.h>

enum { IPHONE_TOUCH_PACKET_MAX_BYTES = 326 };

typedef struct {
    const uint8_t *payload;
    size_t payload_size;
} IphoneTouchPacket;

/* Validate a completed SPI reply. The caller retains ownership of both
 * buffers. This function does not perform SPI I/O. */
int iphone_touch_parse_packet(const uint8_t *header, size_t header_size,
                              const uint8_t *data, size_t data_size,
                              uint8_t expected_tag, IphoneTouchPacket *packet);
