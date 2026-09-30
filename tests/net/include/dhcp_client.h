/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <os/sddf.h>

#define UDP_ECHO_PORT 1235
#define UDP_SEAL_PORT 1236
/* Keep MEMP_NUM_TCP_SEG scaled for lwipopts.h even though we only bind UDP. */
#define TCP_ECHO_MAX_CONNS 1

int setup_udp_socket(void);
