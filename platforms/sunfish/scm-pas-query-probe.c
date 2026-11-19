/* Isolated INFO queries only: no PAS operation, memory buffer, or PAS ID. */
#include <stdint.h>
#include <microkit.h>
#include "scm_pas_query.h"

#define DISPLAY_WIDTH 1080u
#define DISPLAY_HEIGHT 2340u
#define FRAME_BYTES (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4u)
#define SPLASH_MAP_SIZE 0x1600000u
#define FONT_SCALE 7u
#define LINE_HEIGHT 72u

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
	{17,17,17,21,21,21,10}, {17,17,10,4,10,17,17},
	{17,17,10,4,4,4,4}, {31,1,2,4,8,16,31},
	{14,17,19,21,25,17,14}, {4,12,4,4,4,4,14},
	{14,17,1,2,4,8,31}, {30,1,1,14,1,1,30},
	{2,6,10,18,31,2,2}, {31,16,16,30,1,1,30},
	{14,16,16,30,17,17,14}, {31,1,2,4,8,8,8},
	{14,17,17,14,17,17,14}, {14,17,17,15,1,1,14},
};

static uint32_t glyph_index(char c)
{
	if (c >= 'A' && c <= 'Z') return (uint32_t)(c - 'A');
	if (c >= '0' && c <= '9') return 26u + (uint32_t)(c - '0');
	return 36u;
}

static void draw_text(volatile uint32_t *pixels, const char *text,
		      uint32_t x, uint32_t y, uint32_t color)
{
	while (*text != '\0') {
		uint32_t glyph = glyph_index(*text++);
		if (glyph < 36u) {
			for (uint32_t row = 0; row < 7u; ++row) {
				for (uint32_t col = 0; col < 5u; ++col) {
					if ((font[glyph][row] & (1u << (4u - col))) == 0)
						continue;
					for (uint32_t dy = 0; dy < FONT_SCALE; ++dy) {
						for (uint32_t dx = 0; dx < FONT_SCALE; ++dx) {
							uint32_t px = x + col * FONT_SCALE + dx;
							uint32_t py = y + row * FONT_SCALE + dy;
							if (px < DISPLAY_WIDTH && py < DISPLAY_HEIGHT)
								pixels[py * DISPLAY_WIDTH + px] = color;
						}
					}
				}
			}
		}
		x += 6u * FONT_SCALE;
	}
}


static void line(volatile uint32_t *pixels, uint32_t row, const char *text)
{
    draw_text(pixels, text, 48u, 180u + row * LINE_HEIGHT,
              (row & 1u) ? 0x00ffffffu : 0x0000ffffu);
}
static void hex64(volatile uint32_t *pixels, uint32_t row,
                  const char *label, uint64_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    char text[24];
    uint32_t i = 0;
    for (; i < 6u && label[i] != '\0'; ++i) text[i] = label[i];
    for (; i < 7u; ++i) text[i] = ' ';
    for (i = 0; i < 16u; ++i)
        text[7u + i] = digits[(value >> ((15u - i) * 4u)) & 0xfu];
    text[23] = '\0';
    line(pixels, row, text);
}
static void paint(const struct sunfish_scm_query_report *r, bool running)
{
    static const char *const status_labels[] = {"ABIST", "INITST", "MEMST", "AUTHST", "STOPST"};
    static const char *const value_labels[] = {"ABI", "INIT", "MEM", "AUTH", "STOP"};
    static const char *const calling[] = {"CALLING CONVENTION", "CALLING INIT QUERY", "CALLING MEM QUERY", "CALLING AUTH QUERY", "CALLING STOP QUERY"};
    if (splash_vaddr == 0u || 2u * FRAME_BYTES > SPLASH_MAP_SIZE) return;
    struct sunfish_scm_registers request;
    if (!sunfish_scm_query_request(r->last_index, &request)) return;
    volatile uint32_t *base = (volatile uint32_t *)splash_vaddr;
    for (uint32_t frame = 0; frame < 2u; ++frame) {
        volatile uint32_t *pixels = base + frame * (FRAME_BYTES / 4u);
        for (uint32_t i = 0; i < FRAME_BYTES / 4u; ++i) pixels[i] = 0x00081420u;
        line(pixels, 0, "SCM PAS QUERY");
        line(pixels, 1, "READ ONLY AVAILABILITY");
        line(pixels, 2, "NO PAS OPERATION");
        line(pixels, 3, running ? calling[r->last_index] :
             r->error == SUNFISH_SCM_QUERY_OK && r->completed_mask == 31u ? "QUERY DONE" : "QUERY STOPPED");
        for (uint32_t i = 0; i < SUNFISH_SCM_QUERY_COUNT; ++i) {
            if (r->completed_mask & (1u << i)) {
                hex64(pixels, 4u + 2u * i, status_labels[i], r->replies[i].x[0]);
                hex64(pixels, 5u + 2u * i, value_labels[i], r->replies[i].x[1]);
            } else {
                line(pixels, 4u + 2u * i, "NOT COMPLETED");
                line(pixels, 5u + 2u * i, "NOT COMPLETED");
            }
        }
        hex64(pixels, 14, "PASMSK", (r->available_mask >> 1) & 15u);
        hex64(pixels, 15, "ISSUED", r->issued_mask);
        hex64(pixels, 16, "DONE", r->completed_mask);
        hex64(pixels, 17, "ERROR", r->error);
        hex64(pixels, 18, "INDEX", r->last_index);
        hex64(pixels, 19, "QRY FN", request.x[0]);
        hex64(pixels, 20, "TARGET", request.x[2]);
        line(pixels, 21, "SCALAR IDENTIFIERS ONLY");
        line(pixels, 22, "NO BUFFER ARGUMENT");
        line(pixels, 23, "NO RPMH OR AOP WRITES");
        line(pixels, 24, "NO FIRMWARE OR RESET");
        line(pixels, 25, "NO PAS ID PASSED");
        line(pixels, 26, "AVAIL DOES NOT BOOT");
        line(pixels, 28, "RESTART WHEN DONE");
    }
    __asm__ volatile("dsb sy" ::: "memory");
}
static void show_progress(void *context, const struct sunfish_scm_query_report *r)
{
    (void)context;
    paint(r, true);
}
static void invoke(void *context, const struct sunfish_scm_registers *request,
                   struct sunfish_scm_registers *out)
{
    (void)context;
    seL4_ARM_SMCContext args = {
        .x0=request->x[0], .x1=request->x[1], .x2=request->x[2], .x3=request->x[3],
        .x4=request->x[4], .x5=request->x[5], .x6=request->x[6], .x7=request->x[7]
    };
    seL4_ARM_SMCContext reply = {0};
    microkit_arm_smc_call(&args, &reply);
    *out = (struct sunfish_scm_registers){ .x={
        reply.x0, reply.x1, reply.x2, reply.x3, reply.x4, reply.x5, reply.x6, reply.x7
    }};
}
static void debug_hex(uint64_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 60; shift >= 0; shift -= 4)
        microkit_dbg_putc(digits[(value >> shift) & 0xfu]);
}
void init(void)
{
    struct sunfish_scm_query_report r = sunfish_scm_query_run(0, invoke, show_progress);
    paint(&r, false);
    microkit_dbg_puts("SCM-PAS-QUERY raw replies (INFO calls only)\r\n");
    for (uint32_t i = 0; i < SUNFISH_SCM_QUERY_COUNT; ++i) {
        if (!(r.completed_mask & (1u << i))) continue;
        debug_hex(i);
        for (uint32_t j = 0; j < 8; ++j) {
            microkit_dbg_putc(' ');
            debug_hex(r.replies[i].x[j]);
        }
        microkit_dbg_puts("\r\n");
    }
}
void notified(microkit_channel channel) { (void)channel; }
