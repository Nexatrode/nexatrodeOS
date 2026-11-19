/* Read-only Qualcomm SCM IS_CALL_AVAIL query; never calls ASSIGN_MEM itself. */
#include <stdint.h>
#include <microkit.h>

#define PSCI_SYSTEM_RESET 0x84000009u
#define SCM_SVC_INFO 0x06u
#define SCM_SVC_MP 0x0cu
#define SCM_SVC_RTIC 0x19u
#define SCM_SVC_SMMU_PROGRAM 0x15u
#define SCM_IS_CALL_AVAIL 0x01u
#define SCM_MP_RESTORE_SEC_CFG 0x02u
#define SCM_MP_IOMMU_SECURE_PTBL_SIZE 0x03u
#define SCM_MP_IOMMU_SECURE_PTBL_INIT 0x04u
#define SCM_MP_IOMMU_SECURE_MAP2_FLAT 0x12u
#define SCM_MP_IOMMU_SECURE_UNMAP2_FLAT 0x13u
#define SCM_MP_ASSIGN_MEM 0x16u
#define SCM_MP_PREPARE_ATOS 0x21u
#define SCM_SMMU_CHANGE_PAGETABLE_FORMAT 0x01u
#define SCM_RTIC_MEM_PROTECTION 0x04u
#define SCM_SIP_FNID(svc, cmd) \
	(((((svc) & 0xffu) << 8) | ((cmd) & 0xffu)) | 0x02000000u)
#define SMC64_MASK (1ull << 30)
#define SMC_ATOMIC_MASK (1ull << 31)
#define SCM_ARGS_ONE 1ull

#define DISPLAY_WIDTH 1080u
#define DISPLAY_HEIGHT 2340u
#define FRAME_BYTES (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4u)
#define SPLASH_MAP_SIZE 0x1600000u
#define FONT_SCALE 9u
#define LINE_HEIGHT 96u

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

static void paint(const char *line1, const char *line2, const char *line3,
                 const char *line4, const char *line5, const char *line6,
                 const char *line7, const char *line8, const char *line9)
{
	if (splash_vaddr == 0u || 2u * FRAME_BYTES > SPLASH_MAP_SIZE) {
		microkit_dbg_puts("SCM AVAIL SPLASH MAP INVALID\r\n");
		return;
	}
	volatile uint32_t *base = (volatile uint32_t *)splash_vaddr;
	for (uint32_t frame = 0; frame < 2u; ++frame) {
		volatile uint32_t *pixels = base + frame * (FRAME_BYTES / 4u);
		for (uint32_t i = 0; i < FRAME_BYTES / 4u; ++i)
			pixels[i] = 0x00081420u;
		draw_text(pixels, "SCM QUERY", 48u, 180u, 0x00ffffffu);
		draw_text(pixels, line1, 48u, 360u, 0x0000ffffu);
		draw_text(pixels, line2, 48u, 360u + LINE_HEIGHT,
			  0x0000ffffu);
		draw_text(pixels, line3, 48u, 360u + 2u * LINE_HEIGHT,
			  0x0000ffffu);
		draw_text(pixels, line4, 48u, 360u + 3u * LINE_HEIGHT,
			  0x0000ffffu);
		draw_text(pixels, line5, 48u, 360u + 4u * LINE_HEIGHT,
			  0x0000ffffu);
		draw_text(pixels, line6, 48u, 360u + 5u * LINE_HEIGHT,
			  0x0000ffffu);
		draw_text(pixels, line7, 48u, 360u + 6u * LINE_HEIGHT,
			  0x0000ffffu);
		draw_text(pixels, line8, 48u, 360u + 7u * LINE_HEIGHT,
			  0x0000ffffu);
		draw_text(pixels, line9, 48u, 360u + 8u * LINE_HEIGHT,
			  0x0000ffffu);
	}
	__asm__ volatile("dsb sy" ::: "memory");
}

static uint64_t scm_read_only_query(uint32_t service, uint32_t command,
				    uint64_t *status)
{
	seL4_ARM_SMCContext args = {0};
	seL4_ARM_SMCContext response = {0};
	args.x0 = (uint64_t)SCM_SIP_FNID(SCM_SVC_INFO, SCM_IS_CALL_AVAIL) |
		  SMC64_MASK | SMC_ATOMIC_MASK;
	args.x1 = SCM_ARGS_ONE;
	args.x2 = SCM_SIP_FNID(service, command);
	microkit_arm_smc_call(&args, &response);
	*status = response.x0;
	return response.x1;
}

static void show_hex(const char *prefix, uint64_t value)
{
	static const char digits[] = "0123456789ABCDEF";
	microkit_dbg_puts(prefix);
	for (int shift = 60; shift >= 0; shift -= 4)
		microkit_dbg_putc(digits[(value >> shift) & 0xfu]);
	microkit_dbg_puts("\r\n");
}

void init(void)
{
	paint("QUERY RUNNING", "READ ONLY", "DO NOT RESET", "", "", "",
	      "", "", "");
	microkit_dbg_puts("SCM AVAIL QUERY START\r\n");
	uint64_t mp_status = 0, rtic_status = 0;
	uint64_t atos_status = 0, pt_status = 0;
	uint64_t ptbl_size_status = 0, ptbl_init_status = 0;
	uint64_t restore_status = 0, map_status = 0, unmap_status = 0;
	uint64_t mp_available = scm_read_only_query(
		SCM_SVC_MP, SCM_MP_ASSIGN_MEM, &mp_status);
	uint64_t rtic_available = scm_read_only_query(
		SCM_SVC_RTIC, SCM_RTIC_MEM_PROTECTION, &rtic_status);
	uint64_t atos_available = scm_read_only_query(
		SCM_SVC_MP, SCM_MP_PREPARE_ATOS, &atos_status);
	uint64_t pt_available = scm_read_only_query(
		SCM_SVC_SMMU_PROGRAM, SCM_SMMU_CHANGE_PAGETABLE_FORMAT,
		&pt_status);
	uint64_t ptbl_size_available = scm_read_only_query(
		SCM_SVC_MP, SCM_MP_IOMMU_SECURE_PTBL_SIZE, &ptbl_size_status);
	uint64_t ptbl_init_available = scm_read_only_query(
		SCM_SVC_MP, SCM_MP_IOMMU_SECURE_PTBL_INIT, &ptbl_init_status);
	uint64_t restore_available = scm_read_only_query(
		SCM_SVC_MP, SCM_MP_RESTORE_SEC_CFG, &restore_status);
	uint64_t map_available = scm_read_only_query(
		SCM_SVC_MP, SCM_MP_IOMMU_SECURE_MAP2_FLAT, &map_status);
	uint64_t unmap_available = scm_read_only_query(
		SCM_SVC_MP, SCM_MP_IOMMU_SECURE_UNMAP2_FLAT, &unmap_status);
	show_hex("SCM MP ASSIGN_MEM status=", mp_status);
	show_hex("SCM MP ASSIGN_MEM value=", mp_available);
	show_hex("SCM RTIC status=", rtic_status);
	show_hex("SCM RTIC value=", rtic_available);
	show_hex("SCM MP ATOS status=", atos_status);
	show_hex("SCM MP ATOS value=", atos_available);
	show_hex("SCM SMMU PT status=", pt_status);
	show_hex("SCM SMMU PT value=", pt_available);
	show_hex("SCM MP SEC PTBL SIZE status=", ptbl_size_status);
	show_hex("SCM MP SEC PTBL SIZE value=", ptbl_size_available);
	show_hex("SCM MP SEC PTBL INIT status=", ptbl_init_status);
	show_hex("SCM MP SEC PTBL INIT value=", ptbl_init_available);
	show_hex("SCM MP RESTORE SEC CFG status=", restore_status);
	show_hex("SCM MP RESTORE SEC CFG value=", restore_available);
	show_hex("SCM MP SEC MAP2 FLAT status=", map_status);
	show_hex("SCM MP SEC MAP2 FLAT value=", map_available);
	show_hex("SCM MP SEC UNMAP2 FLAT status=", unmap_status);
	show_hex("SCM MP SEC UNMAP2 FLAT value=", unmap_available);
	paint(mp_status == 0u && mp_available == 1u ? "MP ASSIGN YES" :
	      mp_status == 0u ? "MP ASSIGN NO" : "MP ERROR",
	      rtic_status == 0u && rtic_available == 1u ? "RTIC YES" :
	      rtic_status == 0u ? "RTIC NO" : "RTIC ERROR",
	      atos_status == 0u && atos_available == 1u ? "MP ATOS YES" :
	      atos_status == 0u ? "MP ATOS NO" : "ATOS ERROR",
	      pt_status == 0u && pt_available == 1u ? "SMMU PT YES" :
	      pt_status == 0u ? "SMMU PT NO" : "SMMU PT ERROR",
	      ptbl_size_status == 0u && ptbl_size_available == 1u ?
	      "SEC PTBL SIZE YES" :
	      ptbl_size_status == 0u ? "SEC PTBL SIZE NO" : "PTBL SIZE ERROR",
	      ptbl_init_status == 0u && ptbl_init_available == 1u ?
	      "SEC PTBL INIT YES" :
	      ptbl_init_status == 0u ? "SEC PTBL INIT NO" : "PTBL INIT ERROR",
	      restore_status == 0u && restore_available == 1u ?
	      "RESTORE SEC CFG Y" :
	      restore_status == 0u ? "RESTORE SEC CFG N" : "RESTORE CFG ERROR",
	      map_status == 0u && map_available == 1u ? "SEC MAP2 YES" :
	      map_status == 0u ? "SEC MAP2 NO" : "SEC MAP2 ERROR",
	      unmap_status == 0u && unmap_available == 1u ? "SEC UNMAP2 YES" :
	      unmap_status == 0u ? "SEC UNMAP2 NO" : "SEC UNMAP2 ERROR");
	microkit_dbg_puts("SCM AVAIL QUERY DONE; HOLD SCREEN UNTIL RESTART\r\n");
}

void notified(microkit_channel channel)
{
	(void)channel;
}
