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
#define DISPLAY_WIDTH 1080u
#define DISPLAY_HEIGHT 2340u
#define FRAME_BYTES (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4u)
#define SPLASH_MAP_SIZE 0x1600000u

uintptr_t splash_vaddr;
uintptr_t shared_vaddr;

struct shared_queue {
	uint32_t magic;
	uint32_t session_id;
	uint32_t produced;
	uint32_t consumed;
	uint32_t overflows;
	uint8_t slots[WLFW_POLICY_IPC_QUEUE_CAPACITY]
		[WLFW_POLICY_IPC_SLOT_SIZE];
};

static const uint8_t font[36][7] = {
	{14,17,17,31,17,17,17}, {30,17,17,30,17,17,30},
	{14,17,16,16,16,17,14}, {30,17,17,17,17,17,30},
	{31,16,16,30,16,16,31}, {31,16,16,30,16,16,16},
	{14,17,16,23,17,17,15}, {17,17,17,31,17,17,17},
	{14,4,4,4,4,4,14}, {7,2,2,2,18,18,12},
	{17,18,20,24,20,18,17}, {16,16,16,16,16,16,31},
	{17,27,21,21,17,17,17}, {17,25,21,19,17,17,17},
	{14,17,17,17,17,17,14}, {30,17,17,30,16,16,16},
	{14,17,17,17,21,18,13}, {30,17,17,30,20,18,17},
	{15,16,16,14,1,1,30}, {31,4,4,4,4,4,4},
	{17,17,17,17,17,17,14}, {17,17,17,17,17,10,4},
	{17,17,17,21,21,21,10}, {17,17,10,4,10,17,17},
	{17,17,10,4,4,4,4}, {31,1,2,4,8,16,31},
	{14,17,19,21,25,17,14}, {4,12,4,4,4,4,14},
	{14,17,1,2,4,8,31}, {30,1,1,14,1,1,30},
	{2,6,10,18,31,2,2}, {31,16,16,30,1,1,30},
	{14,16,16,30,17,17,14}, {31,1,2,4,8,8,8},
	{14,17,17,14,17,17,14}, {14,17,17,15,1,1,14},
};

static uint32_t glyph_index(char character)
{
	if (character >= 'A' && character <= 'Z')
		return (uint32_t)(character - 'A');
	if (character >= '0' && character <= '9')
		return 26u + (uint32_t)(character - '0');
	return 36u;
}

static void draw_text(volatile uint32_t *pixels, const char *text,
		      uint32_t x, uint32_t y, uint32_t scale)
{
	while (*text != '\0') {
		uint32_t glyph = glyph_index(*text);
		for (uint32_t row = 0; glyph < 36u && row < 7; ++row) {
			uint8_t bits = font[glyph][row];
			for (uint32_t col = 0; col < 5; ++col) {
				if ((bits & (1u << (4u - col))) == 0)
					continue;
				for (uint32_t dy = 0; dy < scale; ++dy) {
					for (uint32_t dx = 0; dx < scale; ++dx) {
						uint32_t px = x + col * scale + dx;
						uint32_t py = y + row * scale + dy;
						if (px < DISPLAY_WIDTH && py < DISPLAY_HEIGHT)
							pixels[py * DISPLAY_WIDTH + px] = 0x00ffffffu;
					}
				}
			}
		}
		x += 6u * scale;
		++text;
	}
}

static void show_result(const char *status)
{
	if (splash_vaddr == 0u || 2u * FRAME_BYTES > SPLASH_MAP_SIZE) {
		microkit_dbg_puts("WLFW SPLASH MAP INVALID\r\n");
		return;
	}
	volatile uint32_t *base = (volatile uint32_t *)splash_vaddr;
	for (uint32_t buffer = 0; buffer < 2; ++buffer) {
		volatile uint32_t *pixels = base + buffer * (FRAME_BYTES / 4u);
		for (uint32_t i = 0; i < FRAME_BYTES / 4u; ++i)
			pixels[i] = 0x00081420u;
		draw_text(pixels, "WLFW", 40, 160, 12);
		draw_text(pixels, status, 40, 300, 12);
	}
	__asm__ volatile("dsb sy" ::: "memory");
}

static int check_event(uint32_t sequence, uint32_t session_id)
{
	volatile struct shared_queue *shared =
		(volatile struct shared_queue *)shared_vaddr;
	if (shared->magic != QUEUE_MAGIC || shared->produced < sequence)
		return -1;
	uint32_t index = (sequence - 1u) % WLFW_POLICY_IPC_QUEUE_CAPACITY;
	uint8_t encoded[WLFW_POLICY_IPC_SLOT_SIZE];
	for (uint32_t i = 0; i < WLFW_POLICY_IPC_SLOT_SIZE; ++i)
		encoded[i] = shared->slots[index][i];
	__asm__ volatile("dsb sy" ::: "memory");
	struct wlfw_policy_event event;
	if (wlfw_policy_event_decode(&event, encoded, sizeof(encoded)) != 0 ||
	    event.session_id != session_id || event.transaction_id != sequence ||
	    event.message_id != WLFW_MSG_MSA_READY || event.payload_len != 7 ||
	    event.payload[0] != 0x02 || event.payload[1] != 0x04 ||
	    event.payload[2] != 0 || event.payload[3] != 0 ||
	    event.payload[4] != 0 || event.payload[5] != 0 ||
	    event.payload[6] != 0)
		return -1;
	return 0;
}

static int enqueue(uint32_t sequence, uint32_t session_id)
{
	microkit_mr_set(0, sequence);
	microkit_mr_set(1, session_id);
	microkit_msginfo reply = microkit_ppcall(
		0, microkit_msginfo_new(PROBE_ENQUEUE, 2));
	volatile struct shared_queue *shared =
		(volatile struct shared_queue *)shared_vaddr;
	if (microkit_msginfo_get_label(reply) != PROBE_ACK ||
	    microkit_msginfo_get_count(reply) != 1 ||
	    microkit_mr_get(0) != sequence || shared->produced != sequence)
		return -1;
	return check_event(sequence, session_id);
}

static int run_queue_test(void)
{
	if (shared_vaddr == 0u)
		return 1;
	volatile struct shared_queue *shared =
		(volatile struct shared_queue *)shared_vaddr;
	for (uint32_t sequence = 1;
	     sequence <= WLFW_POLICY_IPC_QUEUE_CAPACITY; ++sequence) {
		if (enqueue(sequence, 1) != 0)
			return 3;
	}
	microkit_mr_set(0, WLFW_POLICY_IPC_QUEUE_CAPACITY + 1u);
	microkit_mr_set(1, 1);
	microkit_msginfo full = microkit_ppcall(
		0, microkit_msginfo_new(PROBE_ENQUEUE, 2));
	if (microkit_msginfo_get_label(full) != PROBE_FULL ||
	    microkit_msginfo_get_count(full) != 0 ||
	    shared->produced != WLFW_POLICY_IPC_QUEUE_CAPACITY ||
	    shared->overflows != 1u)
		return 4;
	for (uint32_t sequence = 1;
	     sequence <= WLFW_POLICY_IPC_QUEUE_CAPACITY; ++sequence) {
		if (check_event(sequence, 1) != 0)
			return 5;
	}
	microkit_mr_set(0, WLFW_POLICY_IPC_QUEUE_CAPACITY);
	microkit_mr_set(1, 1);
	microkit_msginfo reply = microkit_ppcall(
		0, microkit_msginfo_new(PROBE_CONSUME, 2));
	if (microkit_msginfo_get_label(reply) != PROBE_CONSUMED ||
	    microkit_msginfo_get_count(reply) != 1 ||
	    microkit_mr_get(0) != WLFW_POLICY_IPC_QUEUE_CAPACITY ||
	    shared->consumed != WLFW_POLICY_IPC_QUEUE_CAPACITY)
		return 6;
	if (enqueue(WLFW_POLICY_IPC_QUEUE_CAPACITY + 1u, 1) != 0)
		return 7;
	microkit_mr_set(0, 1);
	reply = microkit_ppcall(0, microkit_msginfo_new(PROBE_NEW_SESSION, 1));
	if (microkit_msginfo_get_label(reply) != PROBE_SESSION_STARTED ||
	    microkit_msginfo_get_count(reply) != 1 || microkit_mr_get(0) != 2 ||
	    shared->session_id != 2 || shared->produced != 0 ||
	    shared->consumed != 0 || shared->overflows != 0)
		return 8;
	for (uint32_t slot = 0; slot < WLFW_POLICY_IPC_QUEUE_CAPACITY; ++slot) {
		for (uint32_t byte = 0; byte < WLFW_POLICY_IPC_SLOT_SIZE; ++byte) {
			if (shared->slots[slot][byte] != 0)
				return 9;
		}
	}
	microkit_mr_set(0, 1);
	microkit_mr_set(1, 1);
	reply = microkit_ppcall(0, microkit_msginfo_new(PROBE_ENQUEUE, 2));
	if (microkit_msginfo_get_label(reply) != PROBE_REJECT ||
	    shared->produced != 0 || shared->session_id != 2)
		return 10;
	if (enqueue(1, 2) != 0)
		return 11;
	return 0;
}

void init(void)
{
	show_result("QUEUE RUN");
	microkit_dbg_puts("WLFW QUEUE TEST START\r\n");
	int result = run_queue_test();
	if (result == 0) {
		show_result("QUEUE PASS");
		microkit_dbg_puts("WLFW QUEUE TEST PASS\r\n");
	} else {
		char status[] = "FAIL 00";
		status[5] = (char)('0' + result / 10);
		status[6] = (char)('0' + result % 10);
		show_result(status);
		microkit_dbg_puts("WLFW QUEUE TEST FAIL\r\n");
	}
}

void notified(microkit_channel channel)
{
	(void)channel;
}
