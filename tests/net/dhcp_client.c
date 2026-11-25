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
uintptr_t haskell_tcp_page;

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
    haskell_tcp_network_ready();
    if (haskell_tcp_page != 0) haskell_dns_configure_server();
    else start_dns_smoke();
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
    if (haskell_tcp_page != 0) {
        volatile struct haskell_tcp_page *pages =
            (volatile struct haskell_tcp_page *)haskell_tcp_page;
        for (unsigned i = 0; i < HASKELL_TCP_SESSIONS; ++i) {
            pages[i].magic = HASKELL_TCP_MAGIC;
            pages[i].request_len = 0;
            pages[i].response_len = 0;
            pages[i].reserved = i;
        }
    }

    sddf_lwip_maybe_notify();
}

#ifdef NEXAWAL_UTM_RX_NOTIFY
static void notify_wallet_rx(void)
{
    if (haskell_tcp_take_wallet_rx_wake()) microkit_notify(WALLET_TCP_CHANNEL);
}
#endif

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
#ifdef NEXAWAL_UTM_RX_NOTIFY
    notify_wallet_rx();
#endif
    sddf_lwip_maybe_notify();
}

microkit_msginfo protected(microkit_channel ch, microkit_msginfo msginfo)
{
#ifdef SIMPLEX_LOCK_LIFECYCLE
    unsigned lifecycle_label = microkit_msginfo_get_label(msginfo);
    if (ch == SIMPLEX_NET_MANAGER_CHANNEL) {
        if (microkit_msginfo_get_count(msginfo) != 1 ||
            (lifecycle_label != SIMPLEX_NET_LOCK && lifecycle_label != SIMPLEX_NET_UNLOCK) ||
            !haskell_simplex_network_transition(lifecycle_label == SIMPLEX_NET_UNLOCK, microkit_mr_get(0)))
            return microkit_msginfo_new(SIMPLEX_NET_DENIED, 0);
        microkit_mr_set(0, haskell_simplex_network_epoch());
        microkit_mr_set(1, haskell_simplex_network_active());
        microkit_mr_set(2, haskell_simplex_network_cleared());
        return microkit_msginfo_new(SIMPLEX_NET_OK, 3);
    }
    if (ch == HASKELL_TCP_CHANNEL && (!haskell_simplex_network_active() ||
        ((volatile struct haskell_tcp_page *)haskell_tcp_page)[0].reserved >= 2))
        return microkit_msginfo_new(HASKELL_TCP_NOT_READY, 0);
#endif
    if ((ch == HASKELL_TCP_CHANNEL || ch == WALLET_TCP_CHANNEL ||
         ch == SHELL_TCP_CHANNEL || ch == WORKER_TCP_CHANNEL
#ifdef DESKTOP_BROWSER_NETWORK
         || ch == BROWSER_TCP_CHANNEL
#endif
         ) &&
        haskell_tcp_page != 0) {
        unsigned label = (unsigned)microkit_msginfo_get_label(msginfo);
        unsigned result = HASKELL_TCP_ERROR;
#ifdef NEXAWAL_UTM_RX_NOTIFY
        bool wants_rx_wake = ch == WALLET_TCP_CHANNEL && label == WALLET_RECV_WIDE &&
                             microkit_msginfo_get_count(msginfo) == 1 &&
                             microkit_mr_get(0) == WALLET_RX_WAKE_ONCE;
#endif
        /* Wallet/worker get dedicated wire pages; agent/shell share page 0. */
#ifdef DESKTOP_BROWSER_NETWORK
        if (ch == BROWSER_TCP_CHANNEL) {
            haskell_tcp_select(HASKELL_TCP_BROWSER_SLOT, true);
        } else
#endif
        if (ch == WALLET_TCP_CHANNEL) {
            haskell_tcp_select(HASKELL_TCP_WALLET_SLOT, true);
        } else if (ch == WORKER_TCP_CHANNEL) {
            haskell_tcp_select(HASKELL_TCP_WORKER_SLOT, true);
        } else {
            volatile struct haskell_tcp_page *base =
                (volatile struct haskell_tcp_page *)haskell_tcp_page;
            uint32_t slot = base[0].reserved;
            if (slot >= HASKELL_TCP_SESSIONS) slot = 0;
            haskell_tcp_select(slot, false);
        }
        /* r18/r19: wallet WaitConnect STATUS PPC runs on this PD and can
         * starve notified(RX). Host lsof showed Cake ESTABLISHED while qemu
         * usernet stayed TCP[SYN_RCVD] until Connection aborted — SYN-ACK was
         * sitting in the virt RX ring. Drain RX before/after TCP ops. */
        sddf_lwip_process_rx();
        if (label == HASKELL_TCP_OPEN) result = haskell_tcp_open();
        else if (label == HASKELL_TCP_STATUS) result = haskell_tcp_status();
        else if (label == HASKELL_TCP_SEND) result = haskell_tcp_send();
        else if (label == HASKELL_TCP_RECV) result = haskell_tcp_recv();
#ifdef NEXAWAL_UTM_WIDE_RX
        else if (label == WALLET_QUERY_RX_WIDE && ch == WALLET_TCP_CHANNEL) result = WALLET_RX_1K;
#ifdef NEXAWAL_UTM_RX_NOTIFY
        else if (label == WALLET_RECV_WIDE && ch == WALLET_TCP_CHANNEL) result = haskell_tcp_recv_wide_wait(wants_rx_wake);
#else
        else if (label == WALLET_RECV_WIDE && ch == WALLET_TCP_CHANNEL) result = haskell_tcp_recv_wide();
#endif
#endif
        else if (label == HASKELL_TCP_CLOSE) result = haskell_tcp_close();
        else if (label == HASKELL_TCP_CANCEL) result = haskell_tcp_cancel();
        else if (label == HASKELL_DNS_START) result = haskell_dns_start();
        else if (label == HASKELL_DNS_POLL) result = haskell_dns_poll();
        /* Non-blocking breadcrumb into active wire page so wallet can observe. */
        if (label == HASKELL_TCP_STATUS) {
            volatile struct haskell_tcp_page *pages =
                (volatile struct haskell_tcp_page *)haskell_tcp_page;
            uint32_t slot = (ch == WALLET_TCP_CHANNEL) ? HASKELL_TCP_WALLET_SLOT
                          : (ch == WORKER_TCP_CHANNEL) ? HASKELL_TCP_WORKER_SLOT
                          : pages[0].reserved;
            if (slot >= HASKELL_TCP_SESSIONS) slot = 0;
#ifdef DESKTOP_BROWSER_NETWORK
            if (ch == BROWSER_TCP_CHANNEL) {
                volatile struct haskell_tcp_page *browser=(void*)browser_tcp_page;
                browser->response_len=1;browser->response[0]=(uint8_t)(result & 0xff);
            } else
#endif
            if (ch == WALLET_TCP_CHANNEL || ch == WORKER_TCP_CHANNEL) {
                pages[slot].response_len = 1;
                pages[slot].response[0] = (uint8_t)(result & 0xff);
            } else {
                pages[0].response_len = 1;
                pages[0].response[0] = (uint8_t)(result & 0xff);
            }
        }
        /* SYN-ACK / data may have arrived during the op; drain again. */
        sddf_lwip_process_rx();
#ifdef NEXAWAL_UTM_RX_NOTIFY
        notify_wallet_rx();
#endif
        /* Kick TX after OPEN/SEND so SYN/data leave without waiting for RX.
         * maybe_notify() from protected() only defers the signal, and this
         * PD's protected return does not flush it, so the SYN sat in the
         * sDDF queue until lwIP aborted. Signal virt TX directly. */
        if (label == HASKELL_TCP_OPEN || label == HASKELL_TCP_SEND
            || label == HASKELL_TCP_STATUS || label == HASKELL_TCP_RECV
#ifdef NEXAWAL_UTM_WIDE_RX
            || label == WALLET_RECV_WIDE
#endif
        ) {
            sddf_lwip_maybe_notify();
            microkit_notify(net_config.tx.id);
        }
#ifdef NEXAWAL_UTM_WIDE_RX
        if (result == WALLET_RX_1K) {
            /* Older clients check only the label and keep requesting 1 KiB. */
            microkit_mr_set(0, WALLET_WIDE_RX_BYTES);
            return microkit_msginfo_new(result, 1);
        }
#endif
        return microkit_msginfo_new(result, 0);
    }
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
