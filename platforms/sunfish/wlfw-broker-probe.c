#include <stdint.h>
#include <microkit.h>
#include "wlfw_policy_ipc.h"

#define PROBE_ENQUEUE 0x510u
#define PROBE_ACK 0x511u
#define PROBE_REJECT 0x512u
#define PROBE_FULL 0x513u
#define PROBE_CONSUME 0x514u
#define PROBE_CONSUMED 0x515u
#define PROBE_NEW_SESSION 0x516u
#define PROBE_SESSION_STARTED 0x517u
#define QUEUE_MAGIC 0x51465557u

struct shared_queue {
	uint32_t magic;
	uint32_t session_id;
	uint32_t produced;
	uint32_t consumed;
	uint32_t overflows;
	uint8_t slots[WLFW_POLICY_IPC_QUEUE_CAPACITY]
		[WLFW_POLICY_IPC_SLOT_SIZE];
};

uintptr_t shared_vaddr;
static int initialized;

static volatile struct shared_queue *queue(void)
{
	return (volatile struct shared_queue *)shared_vaddr;
}

static void initialize_queue(volatile struct shared_queue *shared)
{
	volatile uint8_t *bytes = (volatile uint8_t *)shared;
	for (uint32_t i = 0; i < sizeof(*shared); ++i)
		bytes[i] = 0;
	shared->magic = QUEUE_MAGIC;
	shared->session_id = 1;
	__asm__ volatile("dsb sy" ::: "memory");
	initialized = 1;
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
	if (channel != 0 || shared_vaddr == 0u)
		return microkit_msginfo_new(PROBE_REJECT, 0);
	volatile struct shared_queue *shared = queue();
	if (microkit_msginfo_get_label(request) == PROBE_ENQUEUE &&
	    microkit_msginfo_get_count(request) == 2) {
		if (!initialized)
			initialize_queue(shared);
		uint32_t sequence = microkit_mr_get(0);
		uint32_t session_id = microkit_mr_get(1);
		if (shared->magic != QUEUE_MAGIC || sequence == 0u ||
		    sequence > UINT16_MAX || session_id != shared->session_id ||
		    sequence != shared->produced + 1u)
			return microkit_msginfo_new(PROBE_REJECT, 0);
		if (shared->produced - shared->consumed >=
		    WLFW_POLICY_IPC_QUEUE_CAPACITY) {
			shared->overflows++;
			__asm__ volatile("dsb sy" ::: "memory");
			return microkit_msginfo_new(PROBE_FULL, 0);
		}

		struct wlfw_policy_event event = {
			.session_id = shared->session_id,
			.transaction_id = (uint16_t)sequence,
			.message_id = WLFW_MSG_MSA_READY,
			.payload_len = 7,
			.payload = {0x02, 0x04, 0, 0, 0, 0, 0},
		};
		uint8_t encoded[WLFW_POLICY_IPC_SLOT_SIZE];
		if (wlfw_policy_event_encode(encoded, &event) != 0)
			return microkit_msginfo_new(PROBE_REJECT, 0);
		uint32_t slot = shared->produced % WLFW_POLICY_IPC_QUEUE_CAPACITY;
		for (uint32_t i = 0; i < WLFW_POLICY_IPC_SLOT_SIZE; ++i)
			shared->slots[slot][i] = encoded[i];
		__asm__ volatile("dsb sy" ::: "memory");
		shared->produced++;
		microkit_mr_set(0, sequence);
		return microkit_msginfo_new(PROBE_ACK, 1);
	}

	if (microkit_msginfo_get_label(request) == PROBE_CONSUME &&
	    microkit_msginfo_get_count(request) == 2 && initialized &&
	    shared->magic == QUEUE_MAGIC) {
		uint32_t consumed = microkit_mr_get(0);
		if (consumed < shared->consumed || consumed > shared->produced ||
		    microkit_mr_get(1) != shared->session_id)
			return microkit_msginfo_new(PROBE_REJECT, 0);
		shared->consumed = consumed;
		__asm__ volatile("dsb sy" ::: "memory");
		microkit_mr_set(0, consumed);
		return microkit_msginfo_new(PROBE_CONSUMED, 1);
	}

	if (microkit_msginfo_get_label(request) == PROBE_NEW_SESSION &&
	    microkit_msginfo_get_count(request) == 1 && initialized &&
	    shared->magic == QUEUE_MAGIC && microkit_mr_get(0) == shared->session_id &&
	    shared->session_id != UINT32_MAX) {
		shared->session_id++;
		shared->produced = 0;
		shared->consumed = 0;
		shared->overflows = 0;
		for (uint32_t slot = 0; slot < WLFW_POLICY_IPC_QUEUE_CAPACITY; ++slot) {
			for (uint32_t byte = 0; byte < WLFW_POLICY_IPC_SLOT_SIZE; ++byte)
				shared->slots[slot][byte] = 0;
		}
		__asm__ volatile("dsb sy" ::: "memory");
		microkit_mr_set(0, shared->session_id);
		return microkit_msginfo_new(PROBE_SESSION_STARTED, 1);
	}

	return microkit_msginfo_new(PROBE_REJECT, 0);
}

void init(void)
{
	microkit_dbg_puts("WLFW QUEUE BROKER READY\r\n");
}

void notified(microkit_channel channel)
{
	(void)channel;
}
