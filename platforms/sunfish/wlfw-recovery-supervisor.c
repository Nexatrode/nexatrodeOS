#include <stdint.h>
#include <microkit.h>

#define RECOVERY_SUBMIT 0x610u
#define RECOVERY_ACCEPT 0x611u
#define RECOVERY_REJECT 0x612u
#define RECOVERY_QUEUE_MAGIC 0x51465752u
#define RECOVERY_CONTROL_MAGIC 0x43565752u
#define DISPLAY_WIDTH 1080u
#define DISPLAY_HEIGHT 2340u
#define FRAME_BYTES (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4u)
#define SPLASH_MAP_SIZE 0x1600000u

struct recovery_control {
	uint32_t magic;
	uint32_t session_id;
	uint32_t restart_count;
	uint32_t result;
};

struct broker_queue {
	uint32_t magic;
	uint32_t session_id;
	uint32_t produced;
	uint32_t consumed;
	uint32_t marker;
};

uintptr_t control_vaddr;
uintptr_t queue_vaddr;
uintptr_t splash_vaddr;

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
	{17,27,21,21,17,17,17}, {17,17,10,4,10,17,17},
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

static void show_result(const char *status, uint32_t color)
{
	if (splash_vaddr == 0u || 2u * FRAME_BYTES > SPLASH_MAP_SIZE) {
		microkit_dbg_puts("WLFW RECOVERY SPLASH MAP INVALID\r\n");
		return;
	}
	volatile uint32_t *base = (volatile uint32_t *)splash_vaddr;
	for (uint32_t buffer = 0; buffer < 2; ++buffer) {
		volatile uint32_t *pixels = base + buffer * (FRAME_BYTES / 4u);
		for (uint32_t i = 0; i < FRAME_BYTES / 4u; ++i)
			pixels[i] = color;
		draw_text(pixels, "BROKER RECOVERY", 40, 160, 12);
		draw_text(pixels, status, 40, 300, 12);
	}
	__asm__ volatile("dsb sy" ::: "memory");
}

static volatile struct recovery_control *control_page(void)
{
	return (volatile struct recovery_control *)control_vaddr;
}

static volatile struct broker_queue *queue_page(void)
{
	return (volatile struct broker_queue *)queue_vaddr;
}

static void clear_queue(volatile struct broker_queue *queue, uint32_t session)
{
	queue->magic = RECOVERY_QUEUE_MAGIC;
	queue->session_id = session;
	queue->produced = 0;
	queue->consumed = 0;
	queue->marker = 0;
	__asm__ volatile("dsb sy" ::: "memory");
}

void init(void)
{
	volatile struct recovery_control *control = control_page();
	volatile struct broker_queue *queue = queue_page();
	control->magic = RECOVERY_CONTROL_MAGIC;
	control->session_id = 1u;
	control->restart_count = 0u;
	control->result = 0u;
	clear_queue(queue, 1u);
	show_result("WAITING", 0x00081420u);
	microkit_dbg_puts("WLFW RECOVERY SUPERVISOR READY\r\n");
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
	volatile struct recovery_control *control = control_page();
	volatile struct broker_queue *queue = queue_page();
	if (channel != 0u || microkit_msginfo_get_label(request) != RECOVERY_SUBMIT ||
	    microkit_msginfo_get_count(request) != 2u || control->magic != RECOVERY_CONTROL_MAGIC ||
	    control->restart_count != 1u || microkit_mr_get(0) != control->session_id ||
	    microkit_mr_get(1) != 0x46524553u || queue->magic != RECOVERY_QUEUE_MAGIC ||
	    queue->session_id != control->session_id || queue->produced != 1u ||
	    queue->consumed != 0u || queue->marker != 0x46524553u) {
		control->result = 2u;
		show_result("FAIL", 0x00200020u);
		return microkit_msginfo_new(RECOVERY_REJECT, 0);
	}
	control->result = 1u;
	__asm__ volatile("dsb sy" ::: "memory");
	show_result("PASS", 0x00082418u);
	microkit_dbg_puts("WLFW RECOVERY TEST PASS\r\n");
	return microkit_msginfo_new(RECOVERY_ACCEPT, 0);
}

seL4_Bool fault(microkit_child child, microkit_msginfo msginfo,
		microkit_msginfo *reply_msginfo)
{
	(void)msginfo;
	(void)reply_msginfo;
	volatile struct recovery_control *control = control_page();
	volatile struct broker_queue *queue = queue_page();
	if (child != 1u || control->magic != RECOVERY_CONTROL_MAGIC ||
	    control->session_id != 1u || control->restart_count != 0u ||
	    queue->magic != RECOVERY_QUEUE_MAGIC || queue->session_id != 1u ||
	    queue->produced != 1u || queue->consumed != 0u ||
	    queue->marker != 0x5354414cu) {
		control->result = 2u;
		show_result("FAULT FAIL", 0x00200020u);
		microkit_pd_stop(child);
		return seL4_False;
	}

	/* The supervisor owns this generation and scrubs stale broker output. */
	control->session_id = 2u;
	control->restart_count = 1u;
	clear_queue(queue, control->session_id);
	__asm__ volatile("dsb sy" ::: "memory");
	microkit_dbg_puts("WLFW RECOVERY CHILD FAULT; SESSION ROLLED\r\n");
	microkit_pd_restart(child, 0x200000);
	return seL4_False;
}

void notified(microkit_channel channel)
{
	(void)channel;
}
