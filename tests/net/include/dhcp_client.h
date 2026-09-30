/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <os/sddf.h>

#define UDP_ECHO_PORT 1235
#define UDP_SEAL_PORT 1236
#define UDP_HOST_TX_PORT 1237
#define TCP_ECHO_PORT 1238
#define TCP_ECHO_MAX_CONNS 1

#define MESSAGES_NET_CHANNEL 3
#define MESSAGES_NET_TX_MAX 128
#define MESSAGES_NET_READY_MAGIC 0x3154454eu /* "NET1" LE */

#define REQUEST_NET_TX 0x413
#define REPLY_NET_TX_OK 0x414

int setup_udp_socket(void);
int setup_tcp_socket(void);
void start_dns_smoke(void);
