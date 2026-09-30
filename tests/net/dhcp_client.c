/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Serial-free DHCP + UDP/TCP echo/seal client for merging into Personal OS.
 * Logging uses libsddf_util_debug (Microkit debug putchar), not PL011.
 */
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <microkit.h>
#include <os/sddf.h>
#include <sddf/util/util.h>
#include <sddf/util/printf.h>
#include <sddf/network/lib_sddf_lwip.h>
#include <sddf/network/queue.h>
#include <sddf/network/config.h>
#include <sddf/timer/client.h>
#include <sddf/timer/config.h>
#include "lwip/err.h"
#include "lwip/ip_addr.h"
#include "lwip/pbuf.h"
#include "lwip/udp.h"
#include "dhcp_client.h"

__attribute__((__section__(".timer_client_config"))) timer_client_config_t timer_config;
__attribute__((__section__(".net_client_config"))) net_client_config_t net_config;
__attribute__((__section__(".lib_sddf_lwip_config"))) lib_sddf_lwip_config_t lib_sddf_lwip_config;

net_queue_handle_t net_rx_handle;
net_queue_handle_t net_tx_handle;

uintptr_t messages_net_tx;

#define LWIP_TICK_MS 100
#define PENDING_TX_SLOTS 2

struct messages_net_tx_page {
    uint32_t magic;
    uint32_t length;
    uint8_t dest_ip[4];
    uint16_t dest_port;
    uint16_t reserved;
    uint8_t payload[MESSAGES_NET_TX_MAX];
};

struct pending_tx {
    uint8_t payload[MESSAGES_NET_TX_MAX];
    uint32_t length;
    uint8_t dest_ip[4];
    uint16_t dest_port;
    uint16_t reserved;
};

static struct udp_pcb *udp_tx_socket;
static struct pending_tx pending_tx[PENDING_TX_SLOTS];
static unsigned pending_tx_count;

static int send_datagram_snapshot(const struct pending_tx *tx);
static void flush_pending_tx(void);

void netif_status_callback(char *ip_addr)
{
    /* Matches standalone echo_server wording so ./dev test-net can share markers. */
    sddf_printf("DHCP request finished, IP address for netif %s is: %s\n",
                sddf_get_pd_name(), ip_addr);
    start_dns_smoke();
    /* Fragment-only guest B has no messages_net_tx; skip TLS so the host stub
     * stays free for the merged Personal OS guest. */
    if (messages_net_tx != 0) {
        start_tls_smoke();
    }
    flush_pending_tx();
}

void set_timeout(void)
{
    sddf_timer_set_timeout(timer_config.driver_id, LWIP_TICK_MS * NS_IN_MS);
}

static int send_datagram_snapshot(const struct pending_tx *tx)
{
    if (udp_tx_socket == NULL || tx->length == 0 || tx->length > MESSAGES_NET_TX_MAX) {
        return -1;
    }
    struct pbuf *p = pbuf_alloc(PBUF_TRANSPORT, tx->length, PBUF_RAM);
    if (p == NULL) {
        sddf_dprintf("DHCP_CLIENT|LOG: pbuf_alloc failed for sealed TX\n");
        return -1;
    }
    memcpy(p->payload, tx->payload, tx->length);
    uint8_t a = tx->dest_ip[0];
    uint8_t b = tx->dest_ip[1];
    uint8_t c = tx->dest_ip[2];
    uint8_t d = tx->dest_ip[3];
    uint16_t port = tx->dest_port;
    if ((a | b | c | d) == 0 || port == 0) {
        a = 10;
        b = 0;
        c = 2;
        d = 2;
        port = UDP_HOST_TX_PORT;
    }
    ip_addr_t dest;
    IP_ADDR4(&dest, a, b, c, d);
    err_t error = udp_sendto(udp_tx_socket, p, &dest, port);
    pbuf_free(p);
    if (error != ERR_OK) {
        sddf_dprintf("DHCP_CLIENT|LOG: udp_sendto failed: %s\n", lwip_strerr(error));
        return -1;
    }
    sddf_printf("MSG NET TX\n");

    if ((tx->reserved & MESSAGES_NET_TX_TLS) != 0) {
        if (tls_queue_sealed(tx->payload, tx->length) != 0) {
            sddf_dprintf("DHCP_CLIENT|LOG: TLS sealed queue failed\n");
        }
    }
    return 0;
}

static void flush_pending_tx(void)
{
    while (pending_tx_count > 0) {
        if (send_datagram_snapshot(&pending_tx[0]) != 0) {
            return;
        }
        pending_tx_count -= 1;
        for (unsigned i = 0; i < pending_tx_count; i++) {
            pending_tx[i] = pending_tx[i + 1];
        }
    }
}

static int queue_or_send_from_page(void)
{
    if (messages_net_tx == 0) {
        return -1;
    }
    volatile struct messages_net_tx_page *page =
        (volatile struct messages_net_tx_page *)messages_net_tx;
    uint32_t length = page->length;
    if (length == 0 || length > MESSAGES_NET_TX_MAX) {
        sddf_dprintf("DHCP_CLIENT|LOG: bad sealed TX length\n");
        return -1;
    }

    struct pending_tx snapshot;
    snapshot.length = length;
    snapshot.dest_port = page->dest_port;
    snapshot.reserved = page->reserved;
    memcpy(snapshot.dest_ip, (const void *)page->dest_ip, 4);
    memcpy(snapshot.payload, (const void *)page->payload, length);

    flush_pending_tx();
    if (send_datagram_snapshot(&snapshot) == 0) {
        return 0;
    }
    if (pending_tx_count >= PENDING_TX_SLOTS) {
        sddf_dprintf("DHCP_CLIENT|LOG: sealed TX pending queue full\n");
        return -1;
    }
    pending_tx[pending_tx_count++] = snapshot;
    return 0;
}

void init(void)
{
    net_queue_init(&net_rx_handle, net_config.rx.free_queue.vaddr, net_config.rx.active_queue.vaddr,
                   net_config.rx.num_buffers);
    net_queue_init(&net_tx_handle, net_config.tx.free_queue.vaddr, net_config.tx.active_queue.vaddr,
                   net_config.tx.num_buffers);
    net_buffers_init(&net_tx_handle, 0);

    sddf_lwip_init(&lib_sddf_lwip_config, &net_config, &timer_config, net_rx_handle, net_tx_handle,
                   NULL, NULL, netif_status_callback, NULL, NULL, NULL);
    set_timeout();
    setup_udp_socket();
    setup_tcp_socket();

    udp_tx_socket = udp_new_ip_type(IPADDR_TYPE_V4);
    if (udp_tx_socket == NULL) {
        sddf_dprintf("DHCP_CLIENT|LOG: failed to open TX UDP pcb\n");
    }

    if (messages_net_tx != 0) {
        volatile struct messages_net_tx_page *page =
            (volatile struct messages_net_tx_page *)messages_net_tx;
        page->magic = MESSAGES_NET_READY_MAGIC;
        page->length = 0;
        if ((page->reserved & MESSAGES_NET_TX_PEER_SET) == 0) {
            page->dest_ip[0] = 10;
            page->dest_ip[1] = 0;
            page->dest_ip[2] = 2;
            page->dest_ip[3] = 2;
            page->dest_port = UDP_HOST_TX_PORT;
            page->reserved = 0;
        }
    }

    sddf_lwip_maybe_notify();
}

void notified(sddf_channel ch)
{
    if (ch == net_config.rx.id) {
        sddf_lwip_process_rx();
    } else if (ch == timer_config.driver_id) {
        sddf_lwip_process_timeout();
        set_timeout();
    } else if (ch == net_config.tx.id) {
        /* Nothing to do */
    } else {
        sddf_dprintf("DHCP_CLIENT|LOG: unexpected channel: %u\n", ch);
    }
    sddf_lwip_maybe_notify();
}

microkit_msginfo protected(microkit_channel ch, microkit_msginfo msginfo)
{
    if (ch != MESSAGES_NET_CHANNEL) {
        return microkit_msginfo_new(0, 0);
    }
    seL4_Word label = microkit_msginfo_get_label(msginfo);
    if (label != REQUEST_NET_TX) {
        return microkit_msginfo_new(0, 0);
    }
    if (queue_or_send_from_page() != 0) {
        return microkit_msginfo_new(0, 0);
    }
    return microkit_msginfo_new(REPLY_NET_TX_OK, 0);
}
