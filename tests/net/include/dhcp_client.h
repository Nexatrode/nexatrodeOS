/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <os/sddf.h>
#include "haskell_tcp_protocol.h"
#include "simplex_lock_protocol.h"

#define UDP_ECHO_PORT 1235
#define UDP_SEAL_PORT 1236
#define UDP_HOST_TX_PORT 1237
#define TCP_ECHO_PORT 1238
#define TCP_ECHO_MAX_CONNS 1
#define TLS_SMOKE_PORT 1239

#define MESSAGES_NET_CHANNEL 3
#define MESSAGES_NET_TX_MAX 128
#define MESSAGES_NET_READY_MAGIC 0x3154454eu /* "NET1" LE */

#define REQUEST_NET_TX 0x413
#define REPLY_NET_TX_OK 0x414

extern uintptr_t haskell_tcp_page;
void haskell_tcp_network_ready(void);
void haskell_dns_configure_server(void);
/** Select session + wire page before a TCP/DNS PPC (channel-driven). */
void haskell_tcp_select(uint32_t slot, bool slot_local_wire);
unsigned haskell_dns_start(void);
unsigned haskell_dns_poll(void);
unsigned haskell_tcp_open(void);
unsigned haskell_tcp_status(void);
unsigned haskell_tcp_send(void);
unsigned haskell_tcp_recv(void);
#ifdef NEXAWAL_UTM_WIDE_RX
unsigned haskell_tcp_recv_wide(void);
#ifdef NEXAWAL_UTM_RX_NOTIFY
unsigned haskell_tcp_recv_wide_wait(bool notify);
bool haskell_tcp_take_wallet_rx_wake(void);
#endif
#endif
unsigned haskell_tcp_close(void);
unsigned haskell_tcp_cancel(void);

/* messages_net_tx.reserved: also send the datagram over the open TLS session. */
#define MESSAGES_NET_TX_TLS 0x1u
/* messages_net_tx.reserved: peer was locked by Messages (do not reset on init). */
#define MESSAGES_NET_TX_PEER_SET 0x2u

int setup_udp_socket(void);
int setup_tcp_socket(void);
void start_dns_smoke(void);
void start_tls_smoke(void);
bool tls_session_ready(void);
int tls_queue_sealed(const uint8_t *data, size_t len);
