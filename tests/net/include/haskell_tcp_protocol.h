/* Legacy MR contains wallet/worker pages; agent/shell share page 0.
 * The opt-in desktop browser uses a separate wire page and fixed endpoint. */
#pragma once
#include <stdint.h>

/** Agent TCP on 3; wallet on 4; shell tip-probe on 5; scan_worker on 6. */
#define HASKELL_TCP_CHANNEL 3
#define WALLET_TCP_CHANNEL 4
#define SHELL_TCP_CHANNEL 5
#define WORKER_TCP_CHANNEL 6
#define HASKELL_TCP_MAX 128
/** Slot 2 = wallet PD; slot 3 = scan_worker (agent keeps 0/1). */
#ifdef DESKTOP_BROWSER_NETWORK
#define HASKELL_TCP_SESSIONS 5
#define BROWSER_TCP_CHANNEL 8
#define HASKELL_TCP_BROWSER_SLOT 4u
extern uintptr_t browser_tcp_page;
#ifdef DESKTOP_PRIVATE_TCP
extern uintptr_t wallet_tcp_page;
volatile struct haskell_tcp_page *haskell_tcp_wire_page(void);
#endif
#else
#define HASKELL_TCP_SESSIONS 4
#endif
#define HASKELL_TCP_WALLET_SLOT 2u
#define HASKELL_TCP_WORKER_SLOT 3u
#define HASKELL_TCP_MAGIC 0x31504354u /* "TCP1" little endian */
#define HASKELL_TCP_OPEN 0x421
#define HASKELL_TCP_STATUS 0x422
#define HASKELL_TCP_SEND 0x423
#define HASKELL_TCP_RECV 0x424
#define HASKELL_TCP_CLOSE 0x425
#define HASKELL_TCP_CANCEL 0x426
#ifdef NEXAWAL_UTM_WIDE_RX
#define WALLET_RECV_WIDE 0x427
/* Optional request word for a one-shot notification on the wallet channel. */
#define WALLET_RX_WAKE_ONCE 1
#define WALLET_QUERY_RX_WIDE 0x428
#define WALLET_RX_1K 0x438
#define WALLET_WIDE_RX_OFFSET 2048u
#define WALLET_WIDE_RX_BYTES 2048u
#endif
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
