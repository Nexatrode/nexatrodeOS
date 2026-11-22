/* Shared one-page wire format for one bounded native Haskell TCP session. */
#pragma once
#include <stdint.h>

#define HASKELL_TCP_CHANNEL 4
#define HASKELL_TCP_MAX 128
#define HASKELL_TCP_MAGIC 0x31504354u /* "TCP1" little endian */
#define HASKELL_TCP_OPEN 0x421
#define HASKELL_TCP_STATUS 0x422
#define HASKELL_TCP_SEND 0x423
#define HASKELL_TCP_RECV 0x424
#define HASKELL_TCP_CLOSE 0x425
#define HASKELL_TCP_CANCEL 0x426
#define HASKELL_TCP_ACCEPTED 0x430
#define HASKELL_TCP_PENDING 0x431
#define HASKELL_TCP_CONNECTED 0x432
#define HASKELL_TCP_DATA 0x433
#define HASKELL_TCP_CLOSED 0x434
#define HASKELL_TCP_BUSY 0x435
#define HASKELL_TCP_ERROR 0x436
#define HASKELL_TCP_NOT_READY 0x437
#define HASKELL_DNS_START 0x440
#define HASKELL_DNS_POLL 0x441

struct haskell_tcp_page {
    uint32_t magic;
    uint32_t request_len;
    uint32_t response_len;
    uint32_t reserved;
    uint8_t dest_ip[4];
    uint16_t dest_port;
    uint16_t reserved2;
    uint8_t request[HASKELL_TCP_MAX];
    uint8_t response[HASKELL_TCP_MAX];
};
