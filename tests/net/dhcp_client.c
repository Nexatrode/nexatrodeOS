/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Serial-free DHCP + UDP echo client for merging into Personal OS.
 * Logging uses libsddf_util_debug (Microkit debug putchar), not PL011.
 */
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <os/sddf.h>
#include <sddf/util/util.h>
#include <sddf/util/printf.h>
#include <sddf/network/lib_sddf_lwip.h>
#include <sddf/network/queue.h>
#include <sddf/network/config.h>
#include <sddf/timer/client.h>
#include <sddf/timer/config.h>
#include "lwip/pbuf.h"
#include "dhcp_client.h"

__attribute__((__section__(".timer_client_config"))) timer_client_config_t timer_config;
__attribute__((__section__(".net_client_config"))) net_client_config_t net_config;
__attribute__((__section__(".lib_sddf_lwip_config"))) lib_sddf_lwip_config_t lib_sddf_lwip_config;

net_queue_handle_t net_rx_handle;
net_queue_handle_t net_tx_handle;

#define LWIP_TICK_MS 100

void netif_status_callback(char *ip_addr)
{
    /* Matches standalone echo_server wording so ./dev test-net can share markers. */
    sddf_printf("DHCP request finished, IP address for netif %s is: %s\n",
                sddf_get_pd_name(), ip_addr);
}

void set_timeout(void)
{
    sddf_timer_set_timeout(timer_config.driver_id, LWIP_TICK_MS * NS_IN_MS);
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
