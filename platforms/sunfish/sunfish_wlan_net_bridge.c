/* SPDX-License-Identifier: MIT */

#include "sunfish_wlan_net_bridge.h"

#include <limits.h>
#include <string.h>

#include <sddf/network/constants.h>

#define ETHERNET_HEADER_BYTES 14U

static bool window_valid(sunfish_wlan_net_window_t window, uint32_t count)
{
    if (window.vaddr == NULL || count == 0 || window.buffer_count != count || window.bytes == 0
        || window.io_addr % NET_BUFFER_SIZE != 0) {
        return false;
    }
    uint64_t required = (uint64_t)count * NET_BUFFER_SIZE;
    if (window.bytes < required || window.io_addr > UINT64_MAX - window.bytes) {
        return false;
    }
    uintptr_t base = (uintptr_t)window.vaddr;
    if (window.bytes > UINTPTR_MAX - base) {
        return false;
    }
    return true;
}

static uint8_t *resolve_buffer(sunfish_wlan_net_window_t window, const net_buff_desc_t *desc)
{
    if (desc == NULL || desc->io_or_offset < window.io_addr) {
        return NULL;
    }
    uint64_t offset = desc->io_or_offset - window.io_addr;
    uint64_t required = (uint64_t)window.buffer_count * NET_BUFFER_SIZE;
    if (offset % NET_BUFFER_SIZE != 0 || offset >= required || NET_BUFFER_SIZE > required - offset) {
        return NULL;
    }
    return window.vaddr + (size_t)offset;
}

static void notify_active_if_requested(
    net_queue_handle_t *queue,
    uint8_t channel,
    sunfish_wlan_net_bridge_t *bridge)
{
    if (net_require_signal_active(queue)) {
        net_cancel_signal_active(queue);
        bridge->notify(channel, bridge->notify_cookie);
    }
}

static void notify_free_if_requested(
    net_queue_handle_t *queue,
    uint8_t channel,
    sunfish_wlan_net_bridge_t *bridge)
{
    if (net_require_signal_free(queue)) {
        net_cancel_signal_free(queue);
        bridge->notify(channel, bridge->notify_cookie);
    }
}

sunfish_wlan_net_status_t sunfish_wlan_net_bridge_init(
    sunfish_wlan_net_bridge_t *bridge,
    const net_driver_config_t *config,
    uint8_t rx_decap_mode,
    sunfish_wlan_net_window_t rx_window,
    sunfish_wlan_net_window_t tx_window,
    sunfish_wlan_net_notify_fn notify,
    void *notify_cookie)
{
    if (bridge == NULL || config == NULL || notify == NULL) {
        return SUNFISH_WLAN_NET_INVALID_ARGUMENT;
    }
    if (!net_config_check_magic((void *)config)
        || rx_decap_mode != SUNFISH_WLAN_RX_DECAP_ETHERNET
        || config->virt_rx.num_buffers == 0
        || config->virt_tx.num_buffers == 0 || config->virt_rx.free_queue.vaddr == NULL
        || config->virt_rx.active_queue.vaddr == NULL || config->virt_tx.free_queue.vaddr == NULL
        || config->virt_tx.active_queue.vaddr == NULL
        || !window_valid(rx_window, config->virt_rx.num_buffers)
        || !window_valid(tx_window, config->virt_tx.num_buffers)) {
        return SUNFISH_WLAN_NET_INVALID_CONFIG;
    }

    net_queue_init(
        &bridge->rx_queue,
        config->virt_rx.free_queue.vaddr,
        config->virt_rx.active_queue.vaddr,
        config->virt_rx.num_buffers);
    net_queue_init(
        &bridge->tx_queue,
        config->virt_tx.free_queue.vaddr,
        config->virt_tx.active_queue.vaddr,
        config->virt_tx.num_buffers);
    bridge->rx_window = rx_window;
    bridge->tx_window = tx_window;
    bridge->rx_peer_channel = config->virt_rx.id;
    bridge->tx_peer_channel = config->virt_tx.id;
    bridge->notify = notify;
    bridge->notify_cookie = notify_cookie;
    return SUNFISH_WLAN_NET_OK;
}

sunfish_wlan_net_status_t sunfish_wlan_net_rx_copy(
    sunfish_wlan_net_bridge_t *bridge,
    const uint8_t *frame,
    size_t frame_bytes)
{
    if (bridge == NULL || frame == NULL) {
        return SUNFISH_WLAN_NET_INVALID_ARGUMENT;
    }
    if (frame_bytes < ETHERNET_HEADER_BYTES || frame_bytes > NET_BUFFER_SIZE) {
        return SUNFISH_WLAN_NET_INVALID_FRAME;
    }
    if (net_queue_empty_free(&bridge->rx_queue) || net_queue_full_active(&bridge->rx_queue)) {
        return SUNFISH_WLAN_NET_BACKPRESSURE;
    }

    net_queue_t *free_queue = bridge->rx_queue.free;
    net_buff_desc_t expected = free_queue->buffers[free_queue->head % bridge->rx_queue.capacity];
    if (resolve_buffer(bridge->rx_window, &expected) == NULL) {
        return SUNFISH_WLAN_NET_INVALID_BUFFER;
    }

    net_buff_desc_t buffer;
    if (net_dequeue_free(&bridge->rx_queue, &buffer) != 0 || buffer.io_or_offset != expected.io_or_offset) {
        return SUNFISH_WLAN_NET_QUEUE_FAULT;
    }
    uint8_t *destination = resolve_buffer(bridge->rx_window, &buffer);
    if (destination == NULL) {
        return SUNFISH_WLAN_NET_QUEUE_FAULT;
    }
    memmove(destination, frame, frame_bytes);
    buffer.len = (uint16_t)frame_bytes;
    buffer.oid = 0;
    if (net_enqueue_active(&bridge->rx_queue, buffer) != 0) {
        /* The preflight above and single-producer ownership make this unreachable. */
        return SUNFISH_WLAN_NET_QUEUE_FAULT;
    }
    notify_active_if_requested(&bridge->rx_queue, bridge->rx_peer_channel, bridge);
    return SUNFISH_WLAN_NET_OK;
}

int32_t sunfish_wlan_net_bridge_rx_sink(
    void *bridge_context,
    const uint8_t *frame,
    size_t frame_bytes)
{
    if (bridge_context == NULL) {
        return (int32_t)SUNFISH_WLAN_NET_INVALID_ARGUMENT;
    }
    return (int32_t)sunfish_wlan_net_rx_copy(
        (sunfish_wlan_net_bridge_t *)bridge_context,
        frame,
        frame_bytes);
}

sunfish_wlan_net_status_t sunfish_wlan_net_tx_copy_next(
    sunfish_wlan_net_bridge_t *bridge,
    uint8_t *output,
    size_t output_capacity,
    size_t *frame_bytes)
{
    if (bridge == NULL || output == NULL || frame_bytes == NULL) {
        return SUNFISH_WLAN_NET_INVALID_ARGUMENT;
    }
    if (net_queue_empty_active(&bridge->tx_queue)) {
        return SUNFISH_WLAN_NET_IDLE;
    }
    net_queue_t *active_queue = bridge->tx_queue.active;
    net_buff_desc_t expected = active_queue->buffers[active_queue->head % bridge->tx_queue.capacity];
    if (expected.oid != 0) {
        return SUNFISH_WLAN_NET_INVALID_BUFFER;
    }
    if (expected.len < ETHERNET_HEADER_BYTES || expected.len > NET_BUFFER_SIZE) {
        return SUNFISH_WLAN_NET_INVALID_FRAME;
    }
    if (resolve_buffer(bridge->tx_window, &expected) == NULL) {
        return SUNFISH_WLAN_NET_INVALID_BUFFER;
    }
    if (output_capacity < expected.len) {
        return SUNFISH_WLAN_NET_OUTPUT_TOO_SMALL;
    }
    if (net_queue_full_free(&bridge->tx_queue)) {
        return SUNFISH_WLAN_NET_QUEUE_FAULT;
    }

    net_buff_desc_t buffer;
    if (net_dequeue_active(&bridge->tx_queue, &buffer) != 0
        || buffer.io_or_offset != expected.io_or_offset || buffer.len != expected.len) {
        return SUNFISH_WLAN_NET_QUEUE_FAULT;
    }
    const uint8_t *source = resolve_buffer(bridge->tx_window, &buffer);
    if (source == NULL) {
        return SUNFISH_WLAN_NET_QUEUE_FAULT;
    }
    memmove(output, source, buffer.len);
    *frame_bytes = buffer.len;
    if (net_enqueue_free(&bridge->tx_queue, buffer) != 0) {
        return SUNFISH_WLAN_NET_QUEUE_FAULT;
    }
    notify_free_if_requested(&bridge->tx_queue, bridge->tx_peer_channel, bridge);
    return SUNFISH_WLAN_NET_OK;
}

int32_t sunfish_wlan_net_bridge_tx_source(
    void *bridge_context,
    uint8_t *output,
    size_t output_capacity,
    size_t *frame_bytes)
{
    if (bridge_context == NULL) {
        return (int32_t)SUNFISH_WLAN_NET_INVALID_ARGUMENT;
    }
    return (int32_t)sunfish_wlan_net_tx_copy_next(
        (sunfish_wlan_net_bridge_t *)bridge_context,
        output,
        output_capacity,
        frame_bytes);
}
