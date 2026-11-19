/* Read-only probe of the Pixel 4a Apps SMMU stream routes at ABL handoff. */
#include <stdint.h>
#include <microkit.h>

uintptr_t apps_smmu_vaddr;
uintptr_t splash_vaddr;

#define SMMU_ID0 0x020u
#define SMMU_ID1 0x024u
#define SMMU_ID2 0x028u
#define SMMU_ID7 0x03cu
#define SMMU_GLOBAL_FSR 0x048u
#define SMMU_GLOBAL_FSYNR0 0x050u
#define SMMU_GLOBAL_FSYNR1 0x054u
#define SMMU_GLOBAL_FSYNR2 0x058u
#define SMMU_GLOBAL_CFG0 0x000u
#define SMMU_SMR(n) (0x800u + ((n) * 4u))
#define SMMU_S2CR(n) (0xc00u + ((n) * 4u))
#define SMMU_SMR_VALID (1u << 31)
#define SMMU_ID0_SMS (1u << 27)
#define SMMU_ID0_NUMSIDB_SHIFT 9u
#define SMMU_ID0_NUMSMRG_MASK 0xffu
#define SMMU_ID0_NUMSIDB_MASK 0xfu
#define SMMU_ID1_PAGESIZE (1u << 31)
#define SMMU_ID1_NUMPAGENDXB_SHIFT 28u
#define SMMU_ID1_NUMPAGENDXB_MASK 0x7u
#define SMMU_ID1_NUMCB_SHIFT 0u
#define SMMU_ID1_NUMCB_MASK 0xffu
#define SMMU_ID2_IAS_SHIFT 0u
#define SMMU_ID2_IAS_MASK 0xfu
#define SMMU_ID2_OAS_SHIFT 4u
#define SMMU_ID2_OAS_MASK 0xfu
#define SMMU_S2CR_TYPE_SHIFT 16u
#define SMMU_S2CR_CBNDX_MASK 0xffu
#define SMMU_SCTLR_M (1u << 0)
#define SMMU_CB_ACTLR 0x004u
#define SMMU_CB_TTBR0 0x20u
#define SMMU_CB_TCR 0x30u
/* ARM SMMUv2 context-bank registers; FSR is only read by this probe. */
#define SMMU_CB_FSR 0x58u
#define SMMU_CB_FAR 0x60u
#define SMMU_CB_FSYNR0 0x68u
#define SMMU_FSR_EF (1u << 4)
#define SMMU_FSR_PF (1u << 3)
#define SMMU_FSR_TF (1u << 1)
#define SMMU_MMIO_SIZE 0x100000u
#define SCREEN_WIDTH 1080u
#define SCREEN_HEIGHT 2340u
#define FRAME_PIXELS (SCREEN_WIDTH * SCREEN_HEIGHT)
#define FRAME_BYTES (FRAME_PIXELS * 4u)
#define SPLASH_MAP_SIZE 0x1600000u
#define FONT_SCALE 5u
#define LINE_HEIGHT 100u
#define MAX_REPORTED_ROUTES 256u
/* The current Sunfish handoff reports 79 groups (raw 0x4f). Capture follows
 * the hardware count; this legacy dense display clips only its printed rows. */
#define MAX_DISPLAY_ROUTES 80u

/* ICNSS requests mask bit 0, so inspect both 0x240 and its 0x241 alias. */
/* UFS SID 0x300 is included to inspect its inherited ABL route and CB state. */
static const uint16_t target_sids[] = {
    0x240u, 0x241u, 0x520u, 0x521u, 0x522u, 0x300u,
};
#define TARGET_SID_COUNT (sizeof(target_sids) / sizeof(target_sids[0]))
static char route_text[TARGET_SID_COUNT][40];
static uint32_t route_count[TARGET_SID_COUNT];
static uint32_t smmr_valid_count;
static uint32_t s2cr_type_count[4];
static uint32_t translated_context_refs;
static uint64_t translated_context_bitmap;
static uint64_t context_cbar_nonzero_bitmap;
static uint64_t context_translation_bitmap;
static uint64_t context_actlr_nonzero_bitmap;
static uint32_t context_actlr_nonzero_count;
struct route_entry {
    uint32_t slot;
    uint32_t smr;
    uint32_t s2cr;
    uint32_t cbar;
    uint32_t sctlr;
    uint32_t actlr;
};
static struct route_entry routes[MAX_REPORTED_ROUTES];
static uint32_t route_entries;

struct context_bank_entry {
    uint32_t cbar;
    uint32_t sctlr;
    uint32_t actlr;
    uint32_t ttbr0_lo;
    uint32_t ttbr0_hi;
    uint32_t ttbr1_hi;
    uint32_t tcr;
    uint32_t fsr;
    uint32_t far;
    uint32_t fsynr0;
};
static struct context_bank_entry context_banks[256];

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

static uint32_t read32(uint32_t offset)
{
    volatile const uint32_t *reg =
        (volatile const uint32_t *)(apps_smmu_vaddr + offset);
    return *reg;
}

static void hex32(uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 28; shift >= 0; shift -= 4) {
        microkit_dbg_putc(digits[(value >> shift) & 0xfu]);
    }
}

static void number(uint32_t value)
{
    char digits[10];
    unsigned count = 0;
    do {
        digits[count++] = (char)('0' + (value % 10u));
        value /= 10u;
    } while (value != 0u && count < sizeof(digits));
    while (count != 0u) {
        microkit_dbg_putc(digits[--count]);
    }
}

static void report_context_bank(uint32_t cb,
                                const struct context_bank_entry *entry)
{
    microkit_dbg_puts("SMMU CB cb=");
    number(cb);
    microkit_dbg_puts(" cbar=");
    hex32(entry->cbar);
    microkit_dbg_puts(" sctlr=");
    hex32(entry->sctlr);
    microkit_dbg_puts(" actlr=");
    hex32(entry->actlr);
    microkit_dbg_puts(" ttbr0=");
    hex32(entry->ttbr0_hi);
    hex32(entry->ttbr0_lo);
    microkit_dbg_puts(" tcr=");
    hex32(entry->tcr);
    microkit_dbg_puts(" fsr=");
    hex32(entry->fsr);
    microkit_dbg_puts(" far=");
    hex32(entry->far);
    microkit_dbg_puts(" fsynr0=");
    hex32(entry->fsynr0);
    microkit_dbg_puts(" tf/pf/ef=");
    number((entry->fsr & SMMU_FSR_TF) != 0u);
    microkit_dbg_putc('/');
    number((entry->fsr & SMMU_FSR_PF) != 0u);
    microkit_dbg_putc('/');
    number((entry->fsr & SMMU_FSR_EF) != 0u);
    microkit_dbg_puts("\r\n");
}

static void key_hex(const char *key, uint32_t value)
{
    microkit_dbg_puts(key);
    microkit_dbg_putc('=');
    hex32(value);
    microkit_dbg_puts("\r\n");
}

static const uint8_t *glyph(char c)
{
    static const uint8_t blank[7] = {0};
    if (c >= 'A' && c <= 'Z') {
        return font[(unsigned)(c - 'A')];
    }
    if (c >= '0' && c <= '9') {
        return font[26u + (unsigned)(c - '0')];
    }
    return blank;
}

static void put_text(char *out, unsigned *at, const char *text)
{
    while (*text != '\0' && *at < 39u) {
        out[(*at)++] = *text++;
    }
    out[*at] = '\0';
}

static void put_sid(char *out, unsigned *at, uint16_t sid)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 12; shift >= 0; shift -= 4) {
        if (*at < 39u) {
            out[(*at)++] = digits[(sid >> shift) & 0xfu];
        }
    }
    out[*at] = '\0';
}

static void put_hex2(char *out, unsigned *at, uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 4; shift >= 0; shift -= 4) {
        if (*at < 39u) {
            out[(*at)++] = digits[(value >> shift) & 0xfu];
        }
    }
    out[*at] = '\0';
}

static void make_metric_line(char *out, const char *label, uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    unsigned at = 0;
    put_text(out, &at, label);
    put_text(out, &at, " ");
    for (int shift = 28; shift >= 0; shift -= 4) {
        if (at < 39u) {
            out[at++] = digits[(value >> shift) & 0xfu];
        }
    }
    out[at] = '\0';
}

static void make_mask_line(char *out, const char *label, uint64_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    unsigned at = 0;
    put_text(out, &at, label);
    put_text(out, &at, " ");
    for (int shift = 60; shift >= 0; shift -= 4) {
        if (at < 39u) {
            out[at++] = digits[(value >> shift) & 0xfu];
        }
    }
    out[at] = '\0';
}

static void make_route_text(uint32_t target, uint16_t sid,
                            uint32_t type, uint32_t cb, uint32_t sctlr)
{
    char *out = route_text[target];
    unsigned at = 0;
    out[0] = '\0';
    put_text(out, &at, "SID ");
    put_sid(out, &at, sid);
    if (type == 0u) {
        put_text(out, &at, " TRANS CB ");
        put_hex2(out, &at, cb);
        put_text(out, &at,
                 (sctlr & SMMU_SCTLR_M) != 0u ? " ON" : " OFF");
    } else if (type == 1u) {
        put_text(out, &at, " BYPASS");
    } else if (type == 2u) {
        put_text(out, &at, " FAULT");
    } else {
        put_text(out, &at, " TYPE?");
    }
}

static void draw_text_scaled(volatile uint32_t *frame, uint32_t x, uint32_t y,
                             const char *text, uint32_t color, uint32_t scale)
{
    while (*text != '\0' && x + (6u * scale) < SCREEN_WIDTH) {
        const uint8_t *rows = glyph(*text++);
        for (uint32_t row = 0; row < 7u; row++) {
            for (uint32_t col = 0; col < 5u; col++) {
                if ((rows[row] & (1u << (4u - col))) == 0u) {
                    continue;
                }
                for (uint32_t dy = 0; dy < scale; dy++) {
                    for (uint32_t dx = 0; dx < scale; dx++) {
                        uint32_t px = x + col * scale + dx;
                        uint32_t py = y + row * scale + dy;
                        if (px < SCREEN_WIDTH && py < SCREEN_HEIGHT) {
                            frame[py * SCREEN_WIDTH + px] = color;
                        }
                    }
                }
            }
        }
        x += 6u * scale;
    }
}

/* Make read progress visible before the final full report is ready. This only
 * writes the existing splash framebuffer; the SMMU mapping stays read-only. */
static void draw_checkpoint(const char *text)
{
    const uint32_t bg = 0x00081420u;
    const uint32_t white = 0x00f0f6ffu;
    const uint32_t cyan = 0x0000e8ffu;
    if (splash_vaddr == 0u || 2u * FRAME_BYTES > SPLASH_MAP_SIZE) {
        microkit_dbg_puts("SMMU DISPLAY MAP INVALID\r\n");
        return;
    }
    for (uint32_t buffer = 0; buffer < 2u; buffer++) {
        volatile uint32_t *frame = (volatile uint32_t *)
            (splash_vaddr + buffer * FRAME_BYTES);
        for (uint32_t y = 0; y < 140u; y++) {
            for (uint32_t x = 0; x < SCREEN_WIDTH; x++) {
                frame[y * SCREEN_WIDTH + x] = bg;
            }
        }
        draw_text_scaled(frame, 40u, 24u, "SMMU HANDOFF", cyan, 4u);
        draw_text_scaled(frame, 40u, 72u, text, white, 4u);
    }
    __asm__ volatile("dsb sy" ::: "memory");
}

static void make_id_line(char *out, const char *label, uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    unsigned at = 0;
    put_text(out, &at, label);
    put_text(out, &at, " ");
    for (int shift = 28; shift >= 0; shift -= 4) {
        if (at < 39u) {
            out[at++] = digits[(value >> shift) & 0xfu];
        }
    }
    out[at] = '\0';
}

static void append_hex8(char *out, unsigned *at, uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 28; shift >= 0; shift -= 4) {
        if (*at < 63u) {
            out[(*at)++] = digits[(value >> shift) & 0xfu];
        }
    }
    out[*at] = '\0';
}

static void append_text64(char *out, unsigned *at, const char *text)
{
    while (*text != '\0' && *at < 63u) {
        out[(*at)++] = *text++;
    }
    out[*at] = '\0';
}

static void append_hex2(char *out, unsigned *at, uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 4; shift >= 0; shift -= 4) {
        if (*at < 63u) {
            out[(*at)++] = digits[(value >> shift) & 0xfu];
        }
    }
    out[*at] = '\0';
}

static void append_hex4(char *out, unsigned *at, uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 12; shift >= 0; shift -= 4) {
        if (*at < 63u) {
            out[(*at)++] = digits[(value >> shift) & 0xfu];
        }
    }
    out[*at] = '\0';
}

static void append_compact_route(char *out, const struct route_entry *entry)
{
    unsigned at = 0;
    uint32_t sid = entry->smr & 0x7fffu;
    uint32_t mask = (entry->smr >> 16) & 0x7fffu;
    uint32_t type = (entry->s2cr >> SMMU_S2CR_TYPE_SHIFT) & 3u;
    uint32_t cb = entry->s2cr & SMMU_S2CR_CBNDX_MASK;
    out[0] = '\0';
    append_text64(out, &at, "S");
    append_hex2(out, &at, entry->slot);
    append_text64(out, &at, " SID");
    append_hex4(out, &at, sid);
    append_text64(out, &at, " MASK");
    append_hex4(out, &at, mask);
    append_text64(out, &at, " T");
    out[at++] = (char)('0' + type);
    out[at] = '\0';
    append_text64(out, &at, " CB");
    append_hex2(out, &at, cb);
    append_text64(out, &at, " A");
    append_hex8(out, &at, entry->cbar);
    append_text64(out, &at, " X");
    append_hex8(out, &at, entry->sctlr);
    append_text64(out, &at, " Y");
    append_hex8(out, &at, entry->actlr);
}

static void __attribute__((unused)) draw_report(uint32_t cfg0, uint32_t global_fsr,
                        uint32_t id0, uint32_t id1, uint32_t num_cb)
{
    const uint32_t bg = 0x00081420u;
    const uint32_t white = 0x00f0f6ffu;
    const uint32_t cyan = 0x0000e8ffu;
    const uint32_t amber = 0x00ffc844u;
    const uint32_t colors[] = {white, cyan, amber, white, cyan, amber};
    char id0_line[40];
    char id1_line[40];
    char metric_lines[10][40];
    char mask_lines[4][40];
    char displayed_line[40];
    make_id_line(id0_line, "ID0", id0);
    make_id_line(id1_line, "ID1", id1);
    make_metric_line(metric_lines[0], "VALID SMR", smmr_valid_count);
    make_metric_line(metric_lines[1], "TRANS S2CR", s2cr_type_count[0]);
    make_metric_line(metric_lines[2], "BYPASS S2CR", s2cr_type_count[1]);
    make_metric_line(metric_lines[3], "FAULT S2CR", s2cr_type_count[2]);
    make_metric_line(metric_lines[4], "OTHER S2CR", s2cr_type_count[3]);
    make_metric_line(metric_lines[5], "TRANS CB REFS", translated_context_refs);
    make_metric_line(metric_lines[6], "CB COUNT", num_cb);
    make_metric_line(metric_lines[7], "SMMU GFSR", global_fsr);
    make_metric_line(metric_lines[8], "SMMU CFG0", cfg0);
    make_metric_line(metric_lines[9], "ACTLR NONZERO", context_actlr_nonzero_count);
    uint32_t displayed_routes = route_entries < MAX_DISPLAY_ROUTES
                                    ? route_entries : MAX_DISPLAY_ROUTES;
    make_metric_line(displayed_line, "SHOW ROUTES", displayed_routes);
    make_mask_line(mask_lines[0], "CB00 TO CB3F ROUTE", translated_context_bitmap);
    make_mask_line(mask_lines[1], "CB00 TO CB3F BAR NZ", context_cbar_nonzero_bitmap);
    make_mask_line(mask_lines[2], "CB00 TO CB3F SCTLR M", context_translation_bitmap);
    make_mask_line(mask_lines[3], "CB00 TO CB3F ACTLR NZ", context_actlr_nonzero_bitmap);

    if (splash_vaddr == 0u || 2u * FRAME_BYTES > SPLASH_MAP_SIZE) {
        microkit_dbg_puts("SMMU DISPLAY MAP INVALID\r\n");
        return;
    }
    for (uint32_t buffer = 0; buffer < 2u; buffer++) {
        volatile uint32_t *frame = (volatile uint32_t *)
            (splash_vaddr + buffer * FRAME_BYTES);
        for (uint32_t i = 0; i < FRAME_PIXELS; i++) {
            frame[i] = bg;
        }
        draw_text_scaled(frame, 40u, 32u, "SMMU HANDOFF", cyan, 4u);
        for (uint32_t i = 0; i < TARGET_SID_COUNT; i++) {
            draw_text_scaled(frame, 40u, 90u + i * 27u,
                             route_text[i], colors[i], 3u);
        }
        for (uint32_t i = 0; i < 10u; i++) {
            draw_text_scaled(frame, 40u, 250u + i * 19u,
                             metric_lines[i], white, 2u);
        }
        draw_text_scaled(frame, 40u, 441u, id0_line, white, 3u);
        draw_text_scaled(frame, 40u, 462u, id1_line, white, 3u);
        draw_text_scaled(frame, 40u, 487u, displayed_line, cyan, 2u);
        draw_text_scaled(frame, 40u, 509u,
                         "S SLOT SID MASK T CB CBAR SCTLR ACTLR", cyan, 2u);
        draw_text_scaled(frame, 40u, 531u,
                         "T0 TRANS T1 BYPASS T2 FAULT T3 OTHER", cyan, 2u);
        for (uint32_t i = 0; i < 4u; i++) {
            draw_text_scaled(frame, 40u, 550u + i * 19u,
                             mask_lines[i], white, 2u);
        }
        for (uint32_t i = 0; i < displayed_routes; i++) {
            char route_line[64];
            append_compact_route(route_line, &routes[i]);
            draw_text_scaled(frame, 40u, 626u + i * 21u,
                             route_line, colors[i % 5u], 2u);
        }
    }
    __asm__ volatile("dsb sy" ::: "memory");
    microkit_dbg_puts("SMMU DISPLAY REPORT WRITTEN\r\n");
}

static int matches_sid(uint32_t smr, uint16_t sid)
{
    uint32_t id = smr & 0x7fffu;
    uint32_t mask = (smr >> 16) & 0x7fffu;
    return ((id ^ sid) & (~mask & 0x7fffu)) == 0u;
}

static void report_route(uint32_t slot, uint32_t smr, uint32_t s2cr,
                         uint16_t requested_sid, uint32_t page_size,
                         uint32_t cb_base, uint32_t num_cb, uint32_t target)
{
    uint32_t type = (s2cr >> SMMU_S2CR_TYPE_SHIFT) & 3u;
    uint32_t cb = s2cr & SMMU_S2CR_CBNDX_MASK;

    microkit_dbg_puts("SMMU ROUTE sid=");
    hex32(requested_sid);
    microkit_dbg_puts(" slot=");
    number(slot);
    microkit_dbg_puts(" smr=");
    hex32(smr);
    microkit_dbg_puts(" s2cr=");
    hex32(s2cr);
    microkit_dbg_puts(" type=");
    number(type);
    microkit_dbg_puts(" cb=");
    number(cb);
    microkit_dbg_puts("\r\n");
    route_count[target]++;

    /* ARM SMMUv2 S2CR type 0 is translation; 1 is bypass; 2 is fault. */
    uint32_t sctlr = 0u;
    if (type == 0u && cb < num_cb) {
        uint32_t cbar = read32(page_size + cb * 4u);
        uint32_t sctlr_offset = cb_base + cb * page_size;
        uint32_t ttbr0_offset = sctlr_offset + SMMU_CB_TTBR0;
        uint32_t tcr_offset = sctlr_offset + SMMU_CB_TCR;

        if (sctlr_offset + SMMU_CB_TCR + 4u <= SMMU_MMIO_SIZE) {
            sctlr = read32(sctlr_offset);
            uint32_t ttbr0_lo = read32(ttbr0_offset);
            uint32_t ttbr0_hi = read32(ttbr0_offset + 4u);
            uint32_t tcr = read32(tcr_offset);
            key_hex("SMMU CBAR", cbar);
            key_hex("SMMU SCTLR", sctlr);
            key_hex("SMMU TTBR0_LO", ttbr0_lo);
            key_hex("SMMU TTBR0_HI", ttbr0_hi);
            key_hex("SMMU TCR", tcr);
            microkit_dbg_puts((sctlr & SMMU_SCTLR_M) != 0u
                                  ? "SMMU CB_TRANSLATION_ON\r\n"
                                  : "SMMU CB_TRANSLATION_OFF\r\n");
        } else {
            microkit_dbg_puts("SMMU CB_REGS_OUT_OF_WINDOW\r\n");
        }
    }
    make_route_text(target, requested_sid, type, cb, sctlr);
}

#ifdef SUNFISH_BANK_CENSUS
static uint32_t raw_smrs[128];
static uint32_t raw_s2crs[128];

/* Evidence for choosing a later experiment, not an ownership or DMA token.
 * Every S2CR participates, even with an invalid SMR. The second pass checks
 * that all route and context control fields used here stayed unchanged. */
static void draw_bank_census(uint32_t cfg0, uint32_t global_fsr,
                             uint32_t id0, uint32_t id1, uint32_t id2,
                             uint32_t num_smr, uint32_t num_cb,
                             uint32_t page_size, uint32_t cb_base)
{
    uint64_t references = 0u, active = 0u, faults = 0u, conflicts = 0u;
    uint32_t stable = 1u;
    for (uint32_t slot = 0; slot < num_smr; slot++) {
        uint32_t s2cr = raw_s2crs[slot];
        uint32_t cb = s2cr & 0xffu;
        if (((s2cr >> 16) & 3u) == 0u && cb < num_cb) {
            references |= 1ull << cb;
        }
        if (read32(SMMU_SMR(slot)) != raw_smrs[slot] ||
            read32(SMMU_S2CR(slot)) != s2cr) {
            stable = 0u;
        }
    }
    for (uint32_t cb = 0; cb < num_cb; cb++) {
        const struct context_bank_entry *entry = &context_banks[cb];
        uint32_t base = cb_base + cb * page_size;
        if ((entry->sctlr & 1u) != 0u) { active |= 1ull << cb; }
        if ((entry->fsr & ~(3u << 9)) != 0u) { faults |= 1ull << cb; }
        if (read32(page_size + cb * 4u) != entry->cbar ||
            read32(base) != entry->sctlr ||
            read32(base + 4u) != entry->actlr ||
            read32(base + 0x20u) != entry->ttbr0_lo ||
            read32(base + 0x24u) != entry->ttbr0_hi ||
            read32(base + 0x2cu) != entry->ttbr1_hi ||
            read32(base + 0x30u) != entry->tcr ||
            read32(base + 0x58u) != entry->fsr) {
            stable = 0u;
        }
        /* Match the native Linux-reference ASID=bank policy. */
        for (uint32_t other = 0; other < num_cb; other++) {
            const struct context_bank_entry *peer = &context_banks[other];
            if (other != cb && (peer->sctlr & 1u) != 0u &&
                ((peer->ttbr0_hi >> 16) == cb || (peer->ttbr1_hi >> 16) == cb)) {
                conflicts |= 1ull << cb;
            }
        }
    }
    if (read32(SMMU_ID0) != id0 || read32(SMMU_ID1) != id1 ||
        read32(SMMU_ID2) != id2 || read32(SMMU_GLOBAL_CFG0) != cfg0 ||
        read32(SMMU_GLOBAL_FSR) != global_fsr) { stable = 0u; }
    uint64_t implemented = (1ull << num_cb) - 1ull;
    uint64_t eligible = implemented & ~(references | active | faults | conflicts | 1ull);
    if (stable == 0u || global_fsr != 0u || (cfg0 & 1u) != 0u) { eligible = 0u; }
    uint32_t first = 0xffffffffu;
    for (uint32_t cb = 1; cb < num_cb; cb++) {
        if ((eligible & (1ull << cb)) != 0u) { first = cb; break; }
    }
    char lines[28][40];
    make_metric_line(lines[0], "ID0", id0);
    make_metric_line(lines[1], "ID1", id1);
    make_metric_line(lines[2], "ID2", id2);
    make_metric_line(lines[3], "SCR0", cfg0);
    make_metric_line(lines[4], "GLOBAL FSR", global_fsr);
    make_metric_line(lines[5], "STABLE", stable);
    make_metric_line(lines[6], "FREE SLOTS", num_smr - smmr_valid_count);
    make_metric_line(lines[7], "REF MASK LOW", (uint32_t)references);
    make_metric_line(lines[8], "REF MASK HIGH", (uint32_t)(references >> 32));
    make_metric_line(lines[9], "ACTIVE MASK LOW", (uint32_t)active);
    make_metric_line(lines[10], "ACTIVE MASK HI", (uint32_t)(active >> 32));
    make_metric_line(lines[11], "FAULT MASK LOW", (uint32_t)faults);
    make_metric_line(lines[12], "FAULT MASK HI", (uint32_t)(faults >> 32));
    make_metric_line(lines[13], "ASID MASK LOW", (uint32_t)conflicts);
    make_metric_line(lines[14], "ASID MASK HIGH", (uint32_t)(conflicts >> 32));
    make_metric_line(lines[15], "FREE MASK LOW", (uint32_t)eligible);
    make_metric_line(lines[16], "FREE MASK HIGH", (uint32_t)(eligible >> 32));
    make_metric_line(lines[17], "FIRST BANK", first);
    const struct context_bank_entry empty = {0};
    const struct context_bank_entry *choice = first < num_cb ? &context_banks[first] : &empty;
    make_metric_line(lines[18], "BANK SCTLR", choice->sctlr);
    make_metric_line(lines[19], "BANK ACTLR", choice->actlr);
    make_metric_line(lines[20], "BANK FSR", choice->fsr);
    make_metric_line(lines[21], "BANK CBAR", choice->cbar);
    make_metric_line(lines[22], "BANK TTBR0 HI", choice->ttbr0_hi);
    make_metric_line(lines[23], "BANK TTBR1 HI", choice->ttbr1_hi);
    for (uint32_t buffer = 0; buffer < 2u; buffer++) {
        volatile uint32_t *frame = (volatile uint32_t *)(splash_vaddr + buffer * FRAME_BYTES);
        for (uint32_t i = 0; i < FRAME_PIXELS; i++) { frame[i] = 0x00081420u; }
        draw_text_scaled(frame, 48u, 180u, "SMMU BANK CENSUS", 0x0000e8ffu, 7u);
        for (uint32_t row = 0; row < 24u; row++) {
            draw_text_scaled(frame, 48u, 252u + row * 72u, lines[row],
                             row % 2u == 0u ? 0x00f0f6ffu : 0x0000e8ffu, 7u);
        }
        draw_text_scaled(frame, 48u, 2052u, "NO REGISTER WRITES", 0x0000e8ffu, 7u);
        draw_text_scaled(frame, 48u, 2124u, "NO MODEM OR WLAN START", 0x00f0f6ffu, 7u);
        draw_text_scaled(frame, 48u, 2196u, "NO SMC OR FLASH", 0x0000e8ffu, 7u);
        draw_text_scaled(frame, 48u, 2268u, "RESTART WHEN DONE", 0x00f0f6ffu, 7u);
    }
    __asm__ volatile("dsb sy" ::: "memory");
    key_hex("CENSUS STABLE", stable);
    key_hex("CENSUS FIRST BANK", first);
}
#endif

void init(void)
{
    draw_checkpoint("READ START");
    microkit_dbg_puts("\r\nSUNFISH SMMU READ START\r\n");

    uint32_t cfg0 = read32(SMMU_GLOBAL_CFG0);
    uint32_t id0 = read32(SMMU_ID0);
    uint32_t id1 = read32(SMMU_ID1);
    uint32_t id2 = read32(SMMU_ID2);
    uint32_t id7 = read32(SMMU_ID7);
    /* Global status is relevant when a stream has no matching S2CR/CB route. */
    uint32_t global_fsr = read32(SMMU_GLOBAL_FSR);
    uint32_t global_fsynr0 = read32(SMMU_GLOBAL_FSYNR0);
    uint32_t global_fsynr1 = read32(SMMU_GLOBAL_FSYNR1);
    uint32_t global_fsynr2 = read32(SMMU_GLOBAL_FSYNR2);
    draw_checkpoint("GLOBAL READ DONE");
    key_hex("SMMU GR0 CFG0", cfg0);
    key_hex("SMMU ID0", id0);
    key_hex("SMMU ID1", id1);
    key_hex("SMMU ID2", id2);
    key_hex("SMMU ID7", id7);
    key_hex("SMMU GLOBAL FSR", global_fsr);
    key_hex("SMMU GLOBAL FSYNR0", global_fsynr0);
    key_hex("SMMU GLOBAL FSYNR1", global_fsynr1);
    key_hex("SMMU GLOBAL FSYNR2", global_fsynr2);

    uint32_t page_size = (id1 & SMMU_ID1_PAGESIZE) != 0u ? 0x10000u : 0x1000u;
    uint32_t page_count = 1u <<
        (((id1 >> SMMU_ID1_NUMPAGENDXB_SHIFT) & SMMU_ID1_NUMPAGENDXB_MASK) + 1u);
    uint32_t cb_base = page_count * page_size;
    uint32_t num_cb = (id1 >> SMMU_ID1_NUMCB_SHIFT) & SMMU_ID1_NUMCB_MASK;
    /* NUMSMRG is the direct count, matching the pinned Sunfish Linux driver. */
    uint32_t num_smr = id0 & SMMU_ID0_NUMSMRG_MASK;
    uint32_t num_sid_bits = (id0 >> SMMU_ID0_NUMSIDB_SHIFT) & SMMU_ID0_NUMSIDB_MASK;
    uint32_t num_indexed_streams = 1u << num_sid_bits;

    microkit_dbg_puts("SMMU PAGE_SIZE=");
    number(page_size);
    microkit_dbg_puts(" CB_COUNT=");
    number(num_cb);
    microkit_dbg_puts("\r\n");

#ifdef SUNFISH_BANK_CENSUS
    if ((id0 & SMMU_ID0_SMS) == 0u || num_smr == 0u || num_smr > 128u ||
        num_cb != 61u || page_size != 0x1000u || cb_base != 0x80000u) {
        draw_checkpoint("CENSUS LAYOUT STOP");
        return;
    }
#endif
    if ((id0 & SMMU_ID0_SMS) != 0u) {
        if (num_smr > 256u) {
            draw_checkpoint("BAD SMR COUNT");
            microkit_dbg_puts("SMMU BAD_SMR_COUNT\r\n");
            return;
        }
        for (uint32_t slot = 0; slot < num_smr; slot++) {
            uint32_t smr = read32(SMMU_SMR(slot));
#ifdef SUNFISH_BANK_CENSUS
            raw_smrs[slot] = smr;
            raw_s2crs[slot] = read32(SMMU_S2CR(slot));
#endif
            if ((smr & SMMU_SMR_VALID) == 0u) {
                continue;
            }
            smmr_valid_count++;
            uint32_t s2cr = read32(SMMU_S2CR(slot));
            uint32_t type = (s2cr >> SMMU_S2CR_TYPE_SHIFT) & 3u;
            s2cr_type_count[type]++;
            if (type == 0u) {
                translated_context_refs++;
                uint32_t cb = s2cr & SMMU_S2CR_CBNDX_MASK;
                if (cb < 64u) {
                    translated_context_bitmap |= 1ull << cb;
                }
            }
            if (route_entries < MAX_REPORTED_ROUTES) {
                struct route_entry *entry = &routes[route_entries++];
                entry->slot = slot;
                entry->smr = smr;
                entry->s2cr = s2cr;
                entry->cbar = 0u;
                entry->sctlr = 0u;
                entry->actlr = 0u;
                uint32_t cb = s2cr & SMMU_S2CR_CBNDX_MASK;
                if (type == 0u && cb < num_cb && num_cb <= 256u &&
                    cb_base + num_cb * page_size <= SMMU_MMIO_SIZE) {
                    entry->cbar = read32(page_size + cb * 4u);
                    entry->sctlr = read32(cb_base + cb * page_size);
                    entry->actlr = read32(cb_base + cb * page_size + SMMU_CB_ACTLR);
                }
                microkit_dbg_puts("SMMU VALID SLOT=");
                number(slot);
                microkit_dbg_puts(" SMR=");
                hex32(smr);
                microkit_dbg_puts(" S2CR=");
                hex32(s2cr);
                microkit_dbg_puts(" CBAR=");
                hex32(entry->cbar);
                microkit_dbg_puts(" SCTLR=");
                hex32(entry->sctlr);
                microkit_dbg_puts(" ACTLR=");
                hex32(entry->actlr);
                microkit_dbg_puts("\r\n");
            }
            for (uint32_t target = 0; target < TARGET_SID_COUNT; target++) {
                if (matches_sid(smr, target_sids[target])) {
                    report_route(slot, smr, s2cr, target_sids[target],
                                 page_size, cb_base, num_cb, target);
                }
            }
        }
    } else {
        microkit_dbg_puts("SMMU INDEXED_STREAM_MATCH\r\n");
        if (num_indexed_streams > 32768u) {
            draw_checkpoint("BAD STREAM COUNT");
            microkit_dbg_puts("SMMU BAD_STREAM_COUNT\r\n");
            return;
        }
        for (uint32_t target = 0; target < TARGET_SID_COUNT; target++) {
            uint32_t sid = target_sids[target];
            if (sid < num_indexed_streams) {
                uint32_t s2cr = read32(SMMU_S2CR(sid));
                uint32_t type = (s2cr >> SMMU_S2CR_TYPE_SHIFT) & 3u;
                s2cr_type_count[type]++;
                if (type == 0u) {
                    translated_context_refs++;
                }
                report_route(sid, 0u, s2cr,
                             target_sids[target], page_size, cb_base, num_cb,
                             target);
            } else {
                microkit_dbg_puts("SMMU SID_OUT_OF_RANGE sid=");
                hex32(sid);
                microkit_dbg_puts("\r\n");
            }
        }
    }
    draw_checkpoint("ROUTE SCAN DONE");

    if (num_cb > 256u || cb_base + num_cb * page_size > SMMU_MMIO_SIZE) {
        draw_checkpoint("BAD CB WINDOW");
        microkit_dbg_puts("SMMU CB WINDOW INVALID\r\n");
        return;
    }
    for (uint32_t cb = 0; cb < num_cb; cb++) {
        uint32_t cbar = read32(page_size + cb * 4u);
        uint32_t base = cb_base + cb * page_size;
        struct context_bank_entry *entry = &context_banks[cb];
        uint32_t sctlr = read32(base);
        uint32_t actlr = read32(base + SMMU_CB_ACTLR);
        entry->cbar = cbar;
        entry->sctlr = sctlr;
        entry->actlr = actlr;
        entry->ttbr0_lo = read32(base + SMMU_CB_TTBR0);
        entry->ttbr0_hi = read32(base + SMMU_CB_TTBR0 + 4u);
        entry->ttbr1_hi = read32(base + 0x2cu);
        entry->tcr = read32(base + SMMU_CB_TCR);
        /* FSR is write-one-to-clear; this probe reads it and never writes it. */
        entry->fsr = read32(base + SMMU_CB_FSR);
        entry->far = read32(base + SMMU_CB_FAR);
        entry->fsynr0 = read32(base + SMMU_CB_FSYNR0);
        if ((sctlr & SMMU_SCTLR_M) != 0u) {
            if (cb < 64u) {
                context_translation_bitmap |= 1ull << cb;
            }
        }
        if (cbar != 0u && cb < 64u) {
            context_cbar_nonzero_bitmap |= 1ull << cb;
        }
        if (actlr != 0u) {
            context_actlr_nonzero_count++;
            if (cb < 64u) {
                context_actlr_nonzero_bitmap |= 1ull << cb;
            }
        }
        report_context_bank(cb, entry);
    }
    draw_checkpoint("CONTEXT SCAN DONE");

    for (uint32_t target = 0; target < TARGET_SID_COUNT; target++) {
        if (route_count[target] == 0u) {
            unsigned at = 0;
            put_text(route_text[target], &at, "SID ");
            put_sid(route_text[target], &at, target_sids[target]);
            put_text(route_text[target], &at, " NO ROUTE");
        } else if (route_count[target] > 1u) {
            unsigned at = 0;
            put_text(route_text[target], &at, "SID ");
            put_sid(route_text[target], &at, target_sids[target]);
            put_text(route_text[target], &at, " MULTIPLE ROUTES");
        }
    }
#ifdef SUNFISH_BANK_CENSUS
    draw_bank_census(cfg0, global_fsr, id0, id1, id2, num_smr, num_cb, page_size, cb_base);
#else
    draw_report(cfg0, global_fsr, id0, id1, num_cb);
#endif

    microkit_dbg_puts("SUNFISH SMMU READ DONE\r\n");
}

void notified(microkit_channel channel) { (void)channel; }
