#include <assert.h>
#include <stdint.h>
#include <string.h>

#include "../platforms/sunfish/wlfw_policy_ipc.h"

struct host_ring {
	uint8_t slots[WLFW_POLICY_IPC_QUEUE_CAPACITY][WLFW_POLICY_IPC_SLOT_SIZE];
	uint64_t produced;
	uint64_t consumed;
};

static int host_ring_publish(struct host_ring *ring,
			     const struct wlfw_policy_event *event)
{
	if (ring->produced - ring->consumed >= WLFW_POLICY_IPC_QUEUE_CAPACITY)
		return -1;
	uint8_t *slot = ring->slots[ring->produced % WLFW_POLICY_IPC_QUEUE_CAPACITY];
	if (wlfw_policy_event_encode(slot, event) != 0)
		return -1;
	ring->produced++;
	return 0;
}

static int host_ring_consume(struct host_ring *ring,
			     struct wlfw_policy_event *event)
{
	if (ring->consumed == ring->produced)
		return -1;
	const uint8_t *slot = ring->slots[ring->consumed % WLFW_POLICY_IPC_QUEUE_CAPACITY];
	if (wlfw_policy_event_decode(event, slot, WLFW_POLICY_IPC_SLOT_SIZE) != 0)
		return -1;
	ring->consumed++;
	return 0;
}

static void test_round_trip_and_zero_padding(void)
{
	uint8_t slot[WLFW_POLICY_IPC_SLOT_SIZE];
	struct wlfw_policy_event input = {
		.session_id = 3,
		.transaction_id = 0x1234,
		.message_id = WLFW_MSG_MSA_INFO,
		.payload_len = 7,
		.payload = { 0x02, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00 },
	};
	struct wlfw_policy_event output;
	assert(wlfw_policy_event_encode(slot, &input) == 0);
	assert(slot[0] == WLFW_POLICY_IPC_VERSION);
	assert(slot[1] == WLFW_POLICY_IPC_KIND_RESPONSE);
	assert(slot[2] == 0x2d && slot[3] == 0x00);
	assert(slot[4] == 0x34 && slot[5] == 0x12);
	assert(slot[6] == 7 && slot[7] == 0);
	assert(wlfw_policy_event_decode(&output, slot, sizeof(slot)) == 0);
	assert(output.session_id == input.session_id);
	assert(output.transaction_id == input.transaction_id);
	assert(output.message_id == input.message_id);
	assert(output.payload_len == input.payload_len);
	assert(memcmp(output.payload, input.payload, input.payload_len) == 0);
	for (size_t i = WLFW_POLICY_IPC_HEADER_SIZE + input.payload_len;
	     i < sizeof(slot); ++i)
		assert(slot[i] == 0);
}

static void test_rejects_unknown_or_oversized_events(void)
{
	uint8_t slot[WLFW_POLICY_IPC_SLOT_SIZE];
	struct wlfw_policy_event event = {
		.session_id = 1,
		.transaction_id = 1,
		.message_id = WLFW_MSG_MSA_READY,
		.payload_len = 8,
	};
	assert(wlfw_policy_event_encode(slot, &event) == -1);
	event.payload_len = 0;
	event.message_id = 0x7fff;
	assert(wlfw_policy_event_encode(slot, &event) == -1);
	event.message_id = WLFW_MSG_MSA_READY;
	event.session_id = 0;
	assert(wlfw_policy_event_encode(slot, &event) == -1);
	event.session_id = 1;
	event.transaction_id = 0;
	assert(wlfw_policy_event_encode(slot, &event) == -1);
}

static void test_decoder_rejects_bad_header_and_padding(void)
{
	uint8_t slot[WLFW_POLICY_IPC_SLOT_SIZE];
	struct wlfw_policy_event input = {
		.session_id = 2,
		.transaction_id = 4,
		.message_id = WLFW_MSG_CAPABILITY,
		.payload_len = 1,
		.payload = { 0xaa },
	};
	struct wlfw_policy_event output;
	assert(wlfw_policy_event_encode(slot, &input) == 0);
	assert(wlfw_policy_event_decode(&output, slot, sizeof(slot) - 1) == -1);
	slot[0]++;
	assert(wlfw_policy_event_decode(&output, slot, sizeof(slot)) == -1);
	slot[0] = WLFW_POLICY_IPC_VERSION;
	slot[WLFW_POLICY_IPC_HEADER_SIZE + input.payload_len] = 1;
	assert(wlfw_policy_event_decode(&output, slot, sizeof(slot)) == -1);
	slot[WLFW_POLICY_IPC_HEADER_SIZE + input.payload_len] = 0;
	slot[12] = 1;
	assert(wlfw_policy_event_decode(&output, slot, sizeof(slot)) == -1);
}

static void test_host_ring_backpressure_fifo_and_wraparound(void)
{
	struct host_ring ring = { 0 };
	struct wlfw_policy_event event = {
		.session_id = 7,
		.message_id = WLFW_MSG_MSA_READY,
		.payload_len = 0,
	};
	for (uint16_t i = 1; i <= WLFW_POLICY_IPC_QUEUE_CAPACITY; ++i) {
		event.transaction_id = i;
		assert(host_ring_publish(&ring, &event) == 0);
	}
	event.transaction_id = 5;
	assert(host_ring_publish(&ring, &event) == -1);
	for (uint16_t i = 1; i <= 2; ++i) {
		struct wlfw_policy_event received;
		assert(host_ring_consume(&ring, &received) == 0);
		assert(received.transaction_id == i);
	}
	event.transaction_id = 5;
	assert(host_ring_publish(&ring, &event) == 0);
	event.transaction_id = 6;
	assert(host_ring_publish(&ring, &event) == 0);
	for (uint16_t expected = 3; expected <= 6; ++expected) {
		struct wlfw_policy_event received;
		assert(host_ring_consume(&ring, &received) == 0);
		assert(received.session_id == 7);
		assert(received.transaction_id == expected);
	}
	struct wlfw_policy_event empty;
	assert(host_ring_consume(&ring, &empty) == -1);
}

int main(void)
{
	test_round_trip_and_zero_padding();
	test_rejects_unknown_or_oversized_events();
	test_decoder_rejects_bad_header_and_padding();
	test_host_ring_backpressure_fifo_and_wraparound();
	return 0;
}
