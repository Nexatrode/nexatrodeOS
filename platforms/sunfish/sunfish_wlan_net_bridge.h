/* SPDX-License-Identifier: MIT */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include <sddf/network/config.h>
#include <sddf/network/queue.h>

typedef enum sunfish_wlan_net_status {
    SUNFISH_WLAN_NET_OK = 0,
    SUNFISH_WLAN_NET_IDLE,
    SUNFISH_WLAN_NET_BACKPRESSURE,
    SUNFISH_WLAN_NET_INVALID_ARGUMENT,
    SUNFISH_WLAN_NET_INVALID_CONFIG,
    SUNFISH_WLAN_NET_INVALID_FRAME,
    SUNFISH_WLAN_NET_INVALID_BUFFER,
    SUNFISH_WLAN_NET_OUTPUT_TOO_SMALL,
    SUNFISH_WLAN_NET_QUEUE_FAULT,
} sunfish_wlan_net_status_t;

/* Pinned QCACLD CFG_TGT_RX_DECAP_MODE value for Ethernet-decapsulated frames. */
#define SUNFISH_WLAN_RX_DECAP_ETHERNET 2U

typedef void (*sunfish_wlan_net_notify_fn)(uint8_t channel, void *cookie);

typedef struct sunfish_wlan_net_window {
    uint8_t *vaddr;
    uint64_t io_addr;
    uint64_t bytes;
    uint32_t buffer_count;
} sunfish_wlan_net_window_t;

typedef struct sunfish_wlan_net_bridge {
    net_queue_handle_t rx_queue;
    net_queue_handle_t tx_queue;
    sunfish_wlan_net_window_t rx_window;
    sunfish_wlan_net_window_t tx_window;
    uint8_t rx_peer_channel;
    uint8_t tx_peer_channel;
    sunfish_wlan_net_notify_fn notify;
    void *notify_cookie;
} sunfish_wlan_net_bridge_t;

/*
 * Bind the driver's sDDF RX/TX queues to their data windows. The caller must
 * map these exact bounded pools into this PD through the system description.
 * TX currently accepts only source region oid 0. This function does not create
 * mappings or DMA access.
 */
sunfish_wlan_net_status_t sunfish_wlan_net_bridge_init(
    sunfish_wlan_net_bridge_t *bridge,
    const net_driver_config_t *config,
    uint8_t rx_decap_mode,
    sunfish_wlan_net_window_t rx_window,
    sunfish_wlan_net_window_t tx_window,
    sunfish_wlan_net_notify_fn notify,
    void *notify_cookie);

/* Copy one Ethernet frame from WLAN-owned memory to an sDDF RX free buffer. */
sunfish_wlan_net_status_t sunfish_wlan_net_rx_copy(
    sunfish_wlan_net_bridge_t *bridge,
    const uint8_t *frame,
    size_t frame_bytes);

/*
 * `EthernetRxSink` callback ABI for the Rust RX pipeline. The bridge copies
 * the borrowed frame before returning and returns the sDDF bridge status as
 * a signed 32-bit value; zero means SUNFISH_WLAN_NET_OK.
 */
int32_t sunfish_wlan_net_bridge_rx_sink(
    void *bridge_context,
    const uint8_t *frame,
    size_t frame_bytes);

/*
 * Copy one sDDF TX frame into caller-owned WLAN staging storage, then return
 * the sDDF buffer to its free queue. `frame_bytes` is untouched on failure.
 */
sunfish_wlan_net_status_t sunfish_wlan_net_tx_copy_next(
    sunfish_wlan_net_bridge_t *bridge,
    uint8_t *output,
    size_t output_capacity,
    size_t *frame_bytes);

/* `SddfTxSourceCallback` ABI used by the Rust WLAN transmit path. */
int32_t sunfish_wlan_net_bridge_tx_source(
    void *bridge_context,
    uint8_t *output,
    size_t output_capacity,
    size_t *frame_bytes);
