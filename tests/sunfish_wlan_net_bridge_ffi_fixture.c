/* SPDX-License-Identifier: MIT */

#include "../platforms/sunfish/sunfish_wlan_net_bridge.h"

#include <stdlib.h>
#include <string.h>

#define TEST_QUEUE_CAPACITY 2U
#define TEST_RX_IO_BASE UINT64_C(0x12000000)
#define TEST_TX_IO_BASE UINT64_C(0x13000000)

typedef struct test_notifications {
    uint32_t count;
    uint8_t last_channel;
} test_notifications_t;

typedef struct test_fixture {
    net_driver_config_t config;
    net_queue_t *rx_free;
    net_queue_t *rx_active;
    net_queue_t *tx_free;
    net_queue_t *tx_active;
    uint8_t *rx_data;
    uint8_t *tx_data;
    test_notifications_t notifications;
    sunfish_wlan_net_bridge_t bridge;
} test_fixture_t;

static net_queue_t *test_queue_new(uint32_t capacity)
{
    return calloc(1, sizeof(net_queue_t) + capacity * sizeof(net_buff_desc_t));
}

static void test_notify(uint8_t channel, void *cookie)
{
    test_notifications_t *notifications = cookie;
    notifications->count++;
    notifications->last_channel = channel;
}

void *sunfish_test_bridge_new(void)
{
    test_fixture_t *fixture = calloc(1, sizeof(*fixture));
    if (fixture == NULL) {
        return NULL;
    }
    fixture->rx_free = test_queue_new(TEST_QUEUE_CAPACITY);
    fixture->rx_active = test_queue_new(TEST_QUEUE_CAPACITY);
    fixture->tx_free = test_queue_new(TEST_QUEUE_CAPACITY);
    fixture->tx_active = test_queue_new(TEST_QUEUE_CAPACITY);
    fixture->rx_data = calloc(TEST_QUEUE_CAPACITY, NET_BUFFER_SIZE);
    fixture->tx_data = calloc(TEST_QUEUE_CAPACITY, NET_BUFFER_SIZE);
    if (fixture->rx_free == NULL || fixture->rx_active == NULL || fixture->tx_free == NULL
        || fixture->tx_active == NULL || fixture->rx_data == NULL || fixture->tx_data == NULL) {
        goto fail;
    }

    memcpy(fixture->config.magic, "sDDF\5", SDDF_NET_MAGIC_LEN);
    fixture->config.virt_rx.free_queue.vaddr = fixture->rx_free;
    fixture->config.virt_rx.active_queue.vaddr = fixture->rx_active;
    fixture->config.virt_rx.num_buffers = TEST_QUEUE_CAPACITY;
    fixture->config.virt_rx.id = 17;
    fixture->config.virt_tx.free_queue.vaddr = fixture->tx_free;
    fixture->config.virt_tx.active_queue.vaddr = fixture->tx_active;
    fixture->config.virt_tx.num_buffers = TEST_QUEUE_CAPACITY;
    fixture->config.virt_tx.id = 19;

    net_queue_handle_t rx_queue;
    net_queue_init(&rx_queue, fixture->rx_free, fixture->rx_active, TEST_QUEUE_CAPACITY);
    for (uint32_t i = 0; i < TEST_QUEUE_CAPACITY; i++) {
        net_buff_desc_t descriptor = { TEST_RX_IO_BASE + (uint64_t)i * NET_BUFFER_SIZE, 0 };
        if (net_enqueue_free(&rx_queue, descriptor) != 0) {
            goto fail;
        }
    }

    sunfish_wlan_net_window_t rx_window = {
        .vaddr = fixture->rx_data,
        .io_addr = TEST_RX_IO_BASE,
        .bytes = (uint64_t)TEST_QUEUE_CAPACITY * NET_BUFFER_SIZE,
        .buffer_count = TEST_QUEUE_CAPACITY,
    };
    sunfish_wlan_net_window_t tx_window = {
        .vaddr = fixture->tx_data,
        .io_addr = TEST_TX_IO_BASE,
        .bytes = (uint64_t)TEST_QUEUE_CAPACITY * NET_BUFFER_SIZE,
        .buffer_count = TEST_QUEUE_CAPACITY,
    };
    if (sunfish_wlan_net_bridge_init(
            &fixture->bridge,
            &fixture->config,
            SUNFISH_WLAN_RX_DECAP_ETHERNET,
            rx_window,
            tx_window,
            test_notify,
            &fixture->notifications)
        != SUNFISH_WLAN_NET_OK) {
        goto fail;
    }
    return fixture;

fail:
    free(fixture->rx_free);
    free(fixture->rx_active);
    free(fixture->tx_free);
    free(fixture->tx_active);
    free(fixture->rx_data);
    free(fixture->tx_data);
    free(fixture);
    return NULL;
}

void sunfish_test_bridge_destroy(void *context)
{
    test_fixture_t *fixture = context;
    if (fixture == NULL) {
        return;
    }
    free(fixture->rx_free);
    free(fixture->rx_active);
    free(fixture->tx_free);
    free(fixture->tx_active);
    free(fixture->rx_data);
    free(fixture->tx_data);
    free(fixture);
}

int32_t sunfish_test_bridge_rx_callback(void *context, const uint8_t *frame, size_t frame_bytes)
{
    test_fixture_t *fixture = context;
    if (fixture == NULL) {
        return (int32_t)SUNFISH_WLAN_NET_INVALID_ARGUMENT;
    }
    return (int32_t)sunfish_wlan_net_bridge_rx_sink(&fixture->bridge, frame, frame_bytes);
}

int32_t sunfish_test_bridge_tx_callback(
    void *context,
    uint8_t *output,
    size_t output_capacity,
    size_t *frame_bytes)
{
    test_fixture_t *fixture = context;
    if (fixture == NULL) {
        return (int32_t)SUNFISH_WLAN_NET_INVALID_ARGUMENT;
    }
    return sunfish_wlan_net_bridge_tx_source(
        &fixture->bridge,
        output,
        output_capacity,
        frame_bytes);
}

int32_t sunfish_test_bridge_queue_tx(void *context, const uint8_t *frame, size_t frame_bytes)
{
    test_fixture_t *fixture = context;
    if (fixture == NULL || frame == NULL) {
        return (int32_t)SUNFISH_WLAN_NET_INVALID_ARGUMENT;
    }
    if (frame_bytes < 14 || frame_bytes > NET_BUFFER_SIZE) {
        return (int32_t)SUNFISH_WLAN_NET_INVALID_FRAME;
    }
    if (net_queue_full_active(&fixture->bridge.tx_queue)) {
        return (int32_t)SUNFISH_WLAN_NET_BACKPRESSURE;
    }

    memmove(fixture->tx_data, frame, frame_bytes);
    net_buff_desc_t buffer = { TEST_TX_IO_BASE, (uint16_t)frame_bytes, 0 };
    if (net_enqueue_active(&fixture->bridge.tx_queue, buffer) != 0) {
        return (int32_t)SUNFISH_WLAN_NET_QUEUE_FAULT;
    }
    return (int32_t)SUNFISH_WLAN_NET_OK;
}

uint32_t sunfish_test_bridge_rx_active_count(void *context)
{
    test_fixture_t *fixture = context;
    return fixture == NULL ? 0 : net_queue_length(fixture->rx_active);
}

uint32_t sunfish_test_bridge_rx_free_count(void *context)
{
    test_fixture_t *fixture = context;
    return fixture == NULL ? 0 : net_queue_length(fixture->rx_free);
}

uint32_t sunfish_test_bridge_tx_active_count(void *context)
{
    test_fixture_t *fixture = context;
    return fixture == NULL ? 0 : net_queue_length(fixture->tx_active);
}

uint32_t sunfish_test_bridge_tx_free_count(void *context)
{
    test_fixture_t *fixture = context;
    return fixture == NULL ? 0 : net_queue_length(fixture->tx_free);
}

uint32_t sunfish_test_bridge_notification_count(void *context)
{
    test_fixture_t *fixture = context;
    return fixture == NULL ? 0 : fixture->notifications.count;
}

uint8_t sunfish_test_bridge_last_notification_channel(void *context)
{
    test_fixture_t *fixture = context;
    return fixture == NULL ? 0 : fixture->notifications.last_channel;
}

int32_t sunfish_test_bridge_copy_rx(
    void *context,
    uint32_t index,
    uint8_t *output,
    size_t capacity,
    size_t *bytes)
{
    test_fixture_t *fixture = context;
    if (fixture == NULL || output == NULL || bytes == NULL
        || index >= net_queue_length(fixture->rx_active)) {
        return -1;
    }
    net_buff_desc_t descriptor = fixture->rx_active->buffers[
        (fixture->rx_active->head + index) % TEST_QUEUE_CAPACITY];
    if (descriptor.io_or_offset < TEST_RX_IO_BASE) {
        return -2;
    }
    uint64_t offset = descriptor.io_or_offset - TEST_RX_IO_BASE;
    if (offset % NET_BUFFER_SIZE != 0 || offset >= TEST_QUEUE_CAPACITY * NET_BUFFER_SIZE
        || descriptor.len > capacity) {
        return -3;
    }
    memcpy(output, fixture->rx_data + offset, descriptor.len);
    *bytes = descriptor.len;
    return 0;
}
