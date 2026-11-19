#ifndef NEXATRODE_SUNFISH_WLFW_POLICY_IPC_H
#define NEXATRODE_SUNFISH_WLFW_POLICY_IPC_H

#include <stddef.h>
#include <stdint.h>

#define WLFW_POLICY_IPC_VERSION 1u
#define WLFW_POLICY_IPC_KIND_RESPONSE 1u
#define WLFW_POLICY_IPC_HEADER_SIZE 16u
#define WLFW_POLICY_IPC_PAYLOAD_MAX 207u
#define WLFW_POLICY_IPC_SLOT_SIZE 224u
#define WLFW_POLICY_IPC_QUEUE_CAPACITY 4u

#define WLFW_MSG_INDICATION_REGISTER 0x0020u
#define WLFW_MSG_MSA_INFO 0x002du
#define WLFW_MSG_MSA_READY 0x002eu
#define WLFW_MSG_CAPABILITY 0x0024u

struct wlfw_policy_event {
	uint32_t session_id;
	uint16_t transaction_id;
	uint16_t message_id;
	uint16_t payload_len;
	uint8_t payload[WLFW_POLICY_IPC_PAYLOAD_MAX];
};

/*
 * Encode one bounded response slot. The payload contains the QMI TLV payload
 * only (not QRTR, GLINK, or the seven-byte QMI message header). All integers
 * in the wire slot are little-endian. Unused and reserved bytes are zero.
 * Return 0 on success and -1 for invalid input. No I/O or allocation occurs.
 */
static inline int wlfw_policy_event_encode(
	uint8_t out[WLFW_POLICY_IPC_SLOT_SIZE],
	const struct wlfw_policy_event *event)
{
	uint16_t operation_limit;
	if (out == NULL || event == NULL || event->session_id == 0 ||
	    event->transaction_id == 0 || event->payload_len > WLFW_POLICY_IPC_PAYLOAD_MAX)
		return -1;

	switch (event->message_id) {
	case WLFW_MSG_INDICATION_REGISTER: operation_limit = 18; break;
	case WLFW_MSG_MSA_INFO: operation_limit = 37; break;
	case WLFW_MSG_MSA_READY: operation_limit = 7; break;
	case WLFW_MSG_CAPABILITY: operation_limit = 207; break;
	default: return -1;
	}
	if (event->payload_len > operation_limit)
		return -1;

	for (size_t i = 0; i < WLFW_POLICY_IPC_SLOT_SIZE; ++i)
		out[i] = 0;
	out[0] = WLFW_POLICY_IPC_VERSION;
	out[1] = WLFW_POLICY_IPC_KIND_RESPONSE;
	out[2] = (uint8_t)event->message_id;
	out[3] = (uint8_t)(event->message_id >> 8);
	out[4] = (uint8_t)event->transaction_id;
	out[5] = (uint8_t)(event->transaction_id >> 8);
	out[6] = (uint8_t)event->payload_len;
	out[7] = (uint8_t)(event->payload_len >> 8);
	for (size_t i = 0; i < 4; ++i)
		out[8 + i] = (uint8_t)(event->session_id >> (8 * i));
	for (size_t i = 0; i < event->payload_len; ++i)
		out[WLFW_POLICY_IPC_HEADER_SIZE + i] = event->payload[i];
	return 0;
}

/* Decode exactly one slot and reject unknown versions, operations, or padding. */
static inline int wlfw_policy_event_decode(
	struct wlfw_policy_event *event,
	const uint8_t *slot,
	size_t slot_len)
{
	uint16_t payload_len;
	uint16_t operation_limit;
	uint32_t session_id = 0;
	uint16_t message_id;
	uint16_t transaction_id;
	if (event == NULL || slot == NULL || slot_len != WLFW_POLICY_IPC_SLOT_SIZE ||
	    slot[0] != WLFW_POLICY_IPC_VERSION ||
	    slot[1] != WLFW_POLICY_IPC_KIND_RESPONSE)
		return -1;

	message_id = (uint16_t)slot[2] | ((uint16_t)slot[3] << 8);
	transaction_id = (uint16_t)slot[4] | ((uint16_t)slot[5] << 8);
	payload_len = (uint16_t)slot[6] | ((uint16_t)slot[7] << 8);
	for (size_t i = 0; i < 4; ++i)
		session_id |= (uint32_t)slot[8 + i] << (8 * i);
	if (slot[12] || slot[13] || slot[14] || slot[15] ||
	    session_id == 0 || transaction_id == 0 ||
	    payload_len > WLFW_POLICY_IPC_PAYLOAD_MAX)
		return -1;

	switch (message_id) {
	case WLFW_MSG_INDICATION_REGISTER: operation_limit = 18; break;
	case WLFW_MSG_MSA_INFO: operation_limit = 37; break;
	case WLFW_MSG_MSA_READY: operation_limit = 7; break;
	case WLFW_MSG_CAPABILITY: operation_limit = 207; break;
	default: return -1;
	}
	if (payload_len > operation_limit)
		return -1;
	for (size_t i = WLFW_POLICY_IPC_HEADER_SIZE + payload_len;
	     i < WLFW_POLICY_IPC_SLOT_SIZE; ++i) {
		if (slot[i] != 0)
			return -1;
	}

	event->session_id = session_id;
	event->transaction_id = transaction_id;
	event->message_id = message_id;
	event->payload_len = payload_len;
	for (size_t i = 0; i < payload_len; ++i)
		event->payload[i] = slot[WLFW_POLICY_IPC_HEADER_SIZE + i];
	for (size_t i = payload_len; i < WLFW_POLICY_IPC_PAYLOAD_MAX; ++i)
		event->payload[i] = 0;
	return 0;
}

#endif
