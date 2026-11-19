#include "../platforms/sunfish/sunfish_wlan_net_bridge.h"

#include <assert.h>
#include <stdlib.h>
#include <string.h>

#define QUEUE_CAPACITY 2U
#define RX_IO_BASE UINT64_C(0x12000000)
#define TX_IO_BASE UINT64_C(0x13000000)

typedef struct notifications {
    uint32_t count;
    uint8_t last_channel;
} notifications_t;

typedef struct fixture {
    net_driver_config_t config;
    net_queue_t *rx_free;
    net_queue_t *rx_active;
    net_queue_t *tx_free;
    net_queue_t *tx_active;
    uint8_t *rx_data;
    uint8_t *tx_data;
    notifications_t notifications;
    sunfish_wlan_net_bridge_t bridge;
} fixture_t;

static net_queue_t *new_queue(uint32_t capacity)
{
    return calloc(1, sizeof(net_queue_t) + capacity * sizeof(net_buff_desc_t));
}

static void notify(uint8_t channel, void *cookie)
{
    notifications_t *notifications = cookie;
    notifications->count++;
    notifications->last_channel = channel;
}

static void fixture_init(fixture_t *f, uint32_t capacity)
{
    memset(f, 0, sizeof(*f));
    f->rx_free = new_queue(capacity);
    f->rx_active = new_queue(capacity);
    f->tx_free = new_queue(capacity);
    f->tx_active = new_queue(capacity);
    f->rx_data = calloc(capacity, NET_BUFFER_SIZE);
    f->tx_data = calloc(capacity, NET_BUFFER_SIZE);
    assert(f->rx_free && f->rx_active && f->tx_free && f->tx_active);
    assert(f->rx_data && f->tx_data);

    memcpy(f->config.magic, "sDDF\5", SDDF_NET_MAGIC_LEN);
    f->config.virt_rx.free_queue.vaddr = f->rx_free;
    f->config.virt_rx.active_queue.vaddr = f->rx_active;
    f->config.virt_rx.num_buffers = (uint16_t)capacity;
    f->config.virt_rx.id = 17;
    f->config.virt_tx.free_queue.vaddr = f->tx_free;
    f->config.virt_tx.active_queue.vaddr = f->tx_active;
    f->config.virt_tx.num_buffers = (uint16_t)capacity;
    f->config.virt_tx.id = 19;
    net_queue_handle_t rx_queue;
    net_queue_init(&rx_queue, f->rx_free, f->rx_active, capacity);
    for (uint32_t i = 0; i < capacity; i++) {
        net_buff_desc_t desc = { RX_IO_BASE + (uint64_t)i * NET_BUFFER_SIZE, 0 };
        assert(net_enqueue_free(&rx_queue, desc) == 0);
    }

    sunfish_wlan_net_window_t rx_window = {
        .vaddr = f->rx_data,
        .io_addr = RX_IO_BASE,
        .bytes = (uint64_t)capacity * NET_BUFFER_SIZE,
        .buffer_count = capacity,
    };
    sunfish_wlan_net_window_t tx_window = {
        .vaddr = f->tx_data,
        .io_addr = TX_IO_BASE,
        .bytes = (uint64_t)capacity * NET_BUFFER_SIZE,
        .buffer_count = capacity,
    };
    assert(sunfish_wlan_net_bridge_init(
               &f->bridge,
               &f->config,
               SUNFISH_WLAN_RX_DECAP_ETHERNET,
               rx_window,
               tx_window,
               notify,
               &f->notifications)
           == SUNFISH_WLAN_NET_OK);
}

static void fixture_destroy(fixture_t *f)
{
    free(f->rx_free);
    free(f->rx_active);
    free(f->tx_free);
    free(f->tx_active);
    free(f->rx_data);
    free(f->tx_data);
}

static void ethernet_frame(uint8_t *frame, size_t bytes, uint8_t seed)
{
    assert(bytes >= 14);
    for (size_t i = 0; i < bytes; i++) {
        frame[i] = (uint8_t)(seed + i);
    }
}

static void test_rx_copy_delivers_and_signals_after_publication(void)
{
    fixture_t f;
    fixture_init(&f, QUEUE_CAPACITY);
    f.rx_active->consumer_signalled = 0;
    uint8_t frame[64];
    ethernet_frame(frame, sizeof(frame), 0x20);

    assert(sunfish_wlan_net_rx_copy(&f.bridge, frame, sizeof(frame)) == SUNFISH_WLAN_NET_OK);
    assert(net_queue_length(f.rx_free) == QUEUE_CAPACITY - 1);
    assert(net_queue_length(f.rx_active) == 1);
    assert(f.notifications.count == 1);
    assert(f.notifications.last_channel == f.config.virt_rx.id);

    net_queue_handle_t rx_queue;
    net_queue_init(&rx_queue, f.rx_free, f.rx_active, QUEUE_CAPACITY);
    net_buff_desc_t delivered;
    assert(net_dequeue_active(&rx_queue, &delivered) == 0);
    assert(delivered.io_or_offset == RX_IO_BASE);
    assert(delivered.len == sizeof(frame));
    assert(delivered.oid == 0);
    assert(memcmp(f.rx_data, frame, sizeof(frame)) == 0);
    fixture_destroy(&f);
}

static void test_rx_sink_callback_copies_and_returns_bridge_status(void)
{
    fixture_t f;
    fixture_init(&f, QUEUE_CAPACITY);
    uint8_t frame[64];
    ethernet_frame(frame, sizeof(frame), 0x28);

    assert(sunfish_wlan_net_bridge_rx_sink(&f.bridge, frame, sizeof(frame))
           == (int32_t)SUNFISH_WLAN_NET_OK);
    assert(net_queue_length(f.rx_active) == 1);
    assert(memcmp(f.rx_data, frame, sizeof(frame)) == 0);

    assert(sunfish_wlan_net_bridge_rx_sink(&f.bridge, frame, 13)
           == (int32_t)SUNFISH_WLAN_NET_INVALID_FRAME);
    assert(sunfish_wlan_net_bridge_rx_sink(NULL, frame, sizeof(frame))
           == (int32_t)SUNFISH_WLAN_NET_INVALID_ARGUMENT);
    assert(net_queue_length(f.rx_active) == 1);
    fixture_destroy(&f);
}

static void test_rx_backpressure_and_invalid_inputs_leave_queues_untouched(void)
{
    fixture_t f;
    fixture_init(&f, QUEUE_CAPACITY);
    uint8_t frame[NET_BUFFER_SIZE + 1];
    ethernet_frame(frame, sizeof(frame), 0x40);
    uint16_t free_head = f.rx_free->head;
    assert(sunfish_wlan_net_rx_copy(&f.bridge, frame, 13) == SUNFISH_WLAN_NET_INVALID_FRAME);
    assert(sunfish_wlan_net_rx_copy(&f.bridge, frame, sizeof(frame)) == SUNFISH_WLAN_NET_INVALID_FRAME);
    assert(f.rx_free->head == free_head);
    assert(net_queue_empty_active(&f.bridge.rx_queue));

    f.rx_free->buffers[f.rx_free->head % QUEUE_CAPACITY].io_or_offset++;
    assert(sunfish_wlan_net_rx_copy(&f.bridge, frame, 64) == SUNFISH_WLAN_NET_INVALID_BUFFER);
    assert(f.rx_free->head == free_head);
    f.rx_free->buffers[f.rx_free->head % QUEUE_CAPACITY].io_or_offset = RX_IO_BASE;

    net_buff_desc_t dummy = { RX_IO_BASE, 64 };
    assert(net_enqueue_active(&f.bridge.rx_queue, dummy) == 0);
    assert(net_enqueue_active(&f.bridge.rx_queue, dummy) == 0);
    assert(sunfish_wlan_net_rx_copy(&f.bridge, frame, 64) == SUNFISH_WLAN_NET_BACKPRESSURE);
    assert(f.rx_free->head == free_head);
    assert(f.notifications.count == 0);
    fixture_destroy(&f);
}

static void test_tx_copy_returns_the_shared_buffer_only_after_copy(void)
{
    fixture_t f;
    fixture_init(&f, QUEUE_CAPACITY);
    uint8_t source[96];
    ethernet_frame(source, sizeof(source), 0x60);
    memcpy(f.tx_data, source, sizeof(source));
    net_buff_desc_t packet = { TX_IO_BASE, sizeof(source) };
    assert(net_enqueue_active(&f.bridge.tx_queue, packet) == 0);
    f.tx_free->consumer_signalled = 0;

    uint8_t output[96];
    size_t output_len = 0xdead;
    assert(sunfish_wlan_net_tx_copy_next(&f.bridge, output, sizeof(source) - 1, &output_len)
           == SUNFISH_WLAN_NET_OUTPUT_TOO_SMALL);
    assert(output_len == 0xdead);
    assert(net_queue_length(f.tx_active) == 1);
    assert(net_queue_empty_free(&f.bridge.tx_queue));
    assert(f.notifications.count == 0);

    assert(sunfish_wlan_net_tx_copy_next(&f.bridge, output, sizeof(output), &output_len)
           == SUNFISH_WLAN_NET_OK);
    assert(output_len == sizeof(source));
    assert(memcmp(output, source, sizeof(source)) == 0);
    assert(net_queue_empty_active(&f.bridge.tx_queue));
    assert(net_queue_length(f.tx_free) == 1);
    assert(f.tx_free->buffers[f.tx_free->head % QUEUE_CAPACITY].io_or_offset == TX_IO_BASE);
    assert(f.notifications.count == 1);
    assert(f.notifications.last_channel == f.config.virt_tx.id);
    fixture_destroy(&f);
}

static void test_malformed_tx_descriptor_is_not_retired(void)
{
    fixture_t f;
    fixture_init(&f, QUEUE_CAPACITY);
    uint8_t output[64];
    size_t output_len = 7;
    net_buff_desc_t packet = { TX_IO_BASE + 1, 64 };
    assert(net_enqueue_active(&f.bridge.tx_queue, packet) == 0);
    assert(sunfish_wlan_net_tx_copy_next(&f.bridge, output, sizeof(output), &output_len)
           == SUNFISH_WLAN_NET_INVALID_BUFFER);
    assert(net_queue_length(f.tx_active) == 1);
    assert(output_len == 7);

    f.tx_active->buffers[f.tx_active->head % QUEUE_CAPACITY].io_or_offset = TX_IO_BASE;
    f.tx_active->buffers[f.tx_active->head % QUEUE_CAPACITY].len = 13;
    assert(sunfish_wlan_net_tx_copy_next(&f.bridge, output, sizeof(output), &output_len)
           == SUNFISH_WLAN_NET_INVALID_FRAME);
    assert(net_queue_length(f.tx_active) == 1);
    assert(f.notifications.count == 0);
    fixture_destroy(&f);
}

static void test_tx_rejects_unmapped_sddf_region_oid(void)
{
    fixture_t f;
    fixture_init(&f, QUEUE_CAPACITY);
    uint8_t output[64];
    size_t output_len = 0;
    net_buff_desc_t packet = { TX_IO_BASE, 64, 1 };
    assert(net_enqueue_active(&f.bridge.tx_queue, packet) == 0);
    assert(sunfish_wlan_net_tx_copy_next(&f.bridge, output, sizeof(output), &output_len)
           == SUNFISH_WLAN_NET_INVALID_BUFFER);
    assert(net_queue_length(f.tx_active) == 1);
    assert(net_queue_empty_free(&f.bridge.tx_queue));
    assert(output_len == 0);
    assert(f.notifications.count == 0);
    fixture_destroy(&f);
}

static void test_bridge_initialization_rejects_invalid_window_and_magic(void)
{
    fixture_t f;
    fixture_init(&f, QUEUE_CAPACITY);
    sunfish_wlan_net_window_t rx_window = {
        .vaddr = f.rx_data,
        .io_addr = RX_IO_BASE + 1,
        .bytes = (uint64_t)QUEUE_CAPACITY * NET_BUFFER_SIZE,
        .buffer_count = QUEUE_CAPACITY,
    };
    sunfish_wlan_net_window_t tx_window = {
        .vaddr = f.tx_data,
        .io_addr = TX_IO_BASE,
        .bytes = (uint64_t)QUEUE_CAPACITY * NET_BUFFER_SIZE,
        .buffer_count = QUEUE_CAPACITY,
    };
    sunfish_wlan_net_bridge_t rejected;
    assert(sunfish_wlan_net_bridge_init(
               &rejected,
               &f.config,
               SUNFISH_WLAN_RX_DECAP_ETHERNET,
               rx_window,
               tx_window,
               notify,
               NULL)
           == SUNFISH_WLAN_NET_INVALID_CONFIG);
    f.config.magic[0] = 'X';
    assert(sunfish_wlan_net_bridge_init(
               &rejected,
               &f.config,
               SUNFISH_WLAN_RX_DECAP_ETHERNET,
               (sunfish_wlan_net_window_t){
                   .vaddr = f.rx_data,
                   .io_addr = RX_IO_BASE,
                   .bytes = (uint64_t)QUEUE_CAPACITY * NET_BUFFER_SIZE,
                   .buffer_count = QUEUE_CAPACITY,
               },
               tx_window,
               notify,
               NULL)
           == SUNFISH_WLAN_NET_INVALID_CONFIG);
    f.config.magic[0] = 's';
    assert(sunfish_wlan_net_bridge_init(
               &rejected,
               &f.config,
               1U,
               (sunfish_wlan_net_window_t){
                   .vaddr = f.rx_data,
                   .io_addr = RX_IO_BASE,
                   .bytes = (uint64_t)QUEUE_CAPACITY * NET_BUFFER_SIZE,
                   .buffer_count = QUEUE_CAPACITY,
               },
               tx_window,
               notify,
               NULL)
           == SUNFISH_WLAN_NET_INVALID_CONFIG);
    fixture_destroy(&f);
}

int main(void)
{
    test_rx_copy_delivers_and_signals_after_publication();
    test_rx_sink_callback_copies_and_returns_bridge_status();
    test_rx_backpressure_and_invalid_inputs_leave_queues_untouched();
    test_tx_copy_returns_the_shared_buffer_only_after_copy();
    test_malformed_tx_descriptor_is_not_retired();
    test_tx_rejects_unmapped_sddf_region_oid();
    test_bridge_initialization_rejects_invalid_window_and_magic();
    return 0;
}
