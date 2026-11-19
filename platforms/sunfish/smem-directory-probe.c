/*
 * Temporary read-only SMEM directory probe for Pixel 4a.
 *
 * Acquires only TCSR mutex 3 while validating directory metadata. It never
 * writes SMEM, touches GLINK descriptor/FIFO contents, rings a mailbox, or
 * starts WLAN. The separately selected SMEM_IPA_ALLOC experiment adds only
 * the bounded item-497 metadata publication in ipa-smem-allocate.inc.
 * Results are sent to UART and the existing splash framebuffer.
 */
#include <stdint.h>
#include <microkit.h>

uintptr_t smem_vaddr;
uintptr_t tcsr_lock_vaddr;
uintptr_t splash_vaddr;

#define SMEM_BYTES 0x200000u
#define SMEM_PTABLE_BYTES 0x1000u
#define SMEM_HEADER_INITIALIZED 192u
#define SMEM_HEADER_RESERVED 204u
#define SMEM_SBL_VERSION (64u + 4u * 7u)
#define SMEM_PTABLE_HEADER 32u
#define SMEM_PTABLE_ENTRY 48u
#define SMEM_PARTITION_HEADER 32u
#define SMEM_PRIVATE_ENTRY 16u
#define SMEM_CANARY 0xa5a5u
#define SMEM_MAX_PARTITIONS 80u
#ifdef SMEM_IPA_ITEM
#define PROBE_ITEM_COUNT 1u
static uint32_t census_attempts;
static uint32_t census_scans;
static uint32_t census_release_sent;
#else
#define PROBE_ITEM_COUNT 5u
#endif
#define SMEM_MAX_SCAN_STEPS 4096u
#define HWLOCK_APPS_OWNER 1u
#define HWLOCK_TRIES 1000000u
#define FB_WIDTH 1080u
#define FB_HEIGHT 2340u
#define FB_FRAME_PIXELS (FB_WIDTH * FB_HEIGHT)
#define FB_SCALE 5u

struct item_result {
    uint32_t found;
    uint32_t offset;
    uint32_t size;
    uint32_t partition;
    uint32_t host0;
    uint32_t host1;
#ifdef SMEM_IPA_ITEM
    uint32_t cached;
#endif
};

struct route_result {
    uint32_t partition_index;
    uint32_t offset;
    uint32_t size;
    uint32_t cacheline;
    uint32_t free_uncached;
    uint32_t free_cached;
    uint32_t global_fallback;
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

static void snapshot_barrier(void)
{
#ifdef SMEM_PROBE_HOST_TEST
    __asm__ volatile("" ::: "memory");
#else
    __asm__ volatile("dsb sy" ::: "memory");
#endif
}

static uint32_t glyph_index(char c)
{
    if (c >= 'A' && c <= 'Z') return (uint32_t)(c - 'A');
    if (c >= '0' && c <= '9') return 26u + (uint32_t)(c - '0');
    return 36u;
}

static uint32_t *framebuffer(uint32_t buffer)
{
    return (uint32_t *)splash_vaddr + buffer * FB_FRAME_PIXELS;
}

static void fill_screen(uint32_t colour)
{
    for (uint32_t buffer = 0; buffer < 2u; ++buffer) {
        volatile uint32_t *pixels = framebuffer(buffer);
        for (uint32_t i = 0; i < FB_FRAME_PIXELS; ++i) pixels[i] = colour;
    }
}

static void draw_text(const char *text, uint32_t x, uint32_t y, uint32_t colour)
{
    for (; *text != '\0'; ++text, x += 6u * FB_SCALE) {
        uint32_t index = glyph_index(*text);
        if (index >= 36u) continue;
        for (uint32_t row = 0; row < 7u; ++row) {
            for (uint32_t col = 0; col < 5u; ++col) {
                if ((font[index][row] & (1u << (4u - col))) == 0u) continue;
                for (uint32_t dy = 0; dy < FB_SCALE; ++dy) {
                    for (uint32_t dx = 0; dx < FB_SCALE; ++dx) {
                        uint32_t px = x + col * FB_SCALE + dx;
                        uint32_t py = y + row * FB_SCALE + dy;
                        for (uint32_t buffer = 0; buffer < 2u; ++buffer) {
                            framebuffer(buffer)[py * FB_WIDTH + px] = colour;
                        }
                    }
                }
            }
        }
    }
}

static void draw_hex(uint32_t value, uint32_t x, uint32_t y, uint32_t colour)
{
    static const char digits[] = "0123456789ABCDEF";
    char out[8];
    for (uint32_t i = 0; i < 8u; ++i) {
        out[i] = digits[(value >> (28u - 4u * i)) & 0xfu];
    }
    for (uint32_t i = 0; i < 8u; ++i) {
        char one[2] = {out[i], '\0'};
        draw_text(one, x + i * 6u * FB_SCALE, y, colour);
    }
}

#if !defined(SMEM_IPA_ALLOC) && !defined(SMEM_IPA_LAYOUT)
static void show_results(const struct item_result results[PROBE_ITEM_COUNT],
                         const struct route_result *route, int error)
{
    const uint32_t bg = 0x00122b52u;
    const uint32_t white = 0x00f4f7ffu;
    const uint32_t cyan = 0x0000e5e5u;
    fill_screen(bg);
#ifdef SMEM_IPA_ITEM
    draw_text("IPA SMEM ITEM 497", 80u, 100u, cyan);
    draw_text(error ? "SMEM CENSUS STOP" : "SMEM METADATA MATCH", 80u, 200u, white);
    const char *labels[] = {"SMEM BASE", "SMEM BYTES", "SCANS DONE", "LOCK ATTEMPTS",
        "RELEASE SENT", "GLOBAL ROUTE", "PART INDEX", "PART OFFSET", "PART BYTES",
        "CACHELINE", "FREE UNCACHED", "FREE CACHED", "ITEM PRESENT", "ITEM OFFSET",
        "ITEM PA", "ITEM BYTES", "ITEM CACHED", "EXPECTED BYTES", "LOCAL ERROR"};
    const uint32_t values[] = {0x86000000u, SMEM_BYTES, census_scans, census_attempts,
        census_release_sent, route->global_fallback, route->partition_index, route->offset,
        route->size, route->cacheline, route->free_uncached, route->free_cached,
        results[0].found, results[0].offset,
        results[0].found ? 0x86000000u + results[0].offset : 0u,
        results[0].size, results[0].cached, 0x2000u, (uint32_t)(error < 0 ? -error : error)};
    for (uint32_t i = 0; i < sizeof(values) / sizeof(values[0]); ++i) {
        draw_text(labels[i], 80u, 320u + i * 80u, i & 1u ? white : cyan);
        draw_hex(values[i], 750u, 320u + i * 80u, i & 1u ? white : cyan);
    }
    draw_text(results[0].found ? "ITEM METADATA ONLY" : "ITEM NOT ALLOCATED", 80u, 1910u, cyan);
    draw_text("NO SMEM WRITE OR ALLOC", 80u, 1990u, white);
    draw_text("NO IPA SMMU DMA OR PAS", 80u, 2070u, cyan);
    draw_text("RESTART WHEN DONE", 80u, 2150u, white);
#else
    draw_text("SMEM DIRECTORY", 180u, 140u, cyan);
    draw_text("READ ONLY GLINK UNTOUCHED", 180u, 240u, white);
    if (error != 0) {
        draw_text("SCAN ERROR", 180u, 330u, white);
        draw_hex((uint32_t)(-error), 510u, 330u, cyan);
    }
    if (route->size != 0u) {
        draw_text(route->global_fallback ? "ROUTE GLOBAL" : "ROUTE HOST 0 1",
                  180u, 400u, cyan);
        draw_text("TABLE INDEX", 180u, 440u, white);
        draw_hex(route->partition_index, 570u, 440u, white);
        draw_text("PART OFFSET", 180u, 510u, white);
        draw_hex(route->offset, 570u, 510u, white);
        draw_text("PART SIZE", 180u, 580u, white);
        draw_hex(route->size, 570u, 580u, white);
        draw_text("CACHELINE", 180u, 650u, white);
        draw_hex(route->cacheline, 570u, 650u, white);
        draw_text("FREE U C", 180u, 720u, white);
        draw_hex(route->free_uncached, 510u, 720u, white);
        draw_hex(route->free_cached, 780u, 720u, white);
    }
    if (error == 0) {
        for (uint32_t i = 0; i < PROBE_ITEM_COUNT; ++i) {
            uint32_t y = 840u + i * 280u;
            const char *label = i == 0u ? "ITEM 478" :
                                i == 1u ? "ITEM 479" :
                                i == 2u ? "ITEM 480" :
                                i == 3u ? "SMP2P IN 435" : "SMP2P OUT 428";
            draw_text(label, 180u, y, cyan);
            if (results[i].found) {
                draw_text("PART", 180u, y + 65u, white);
                draw_hex(results[i].partition, 330u, y + 65u, white);
                draw_text("HOSTS", 180u, y + 125u, white);
                draw_hex(results[i].host0, 360u, y + 125u, white);
                draw_hex(results[i].host1, 630u, y + 125u, white);
                draw_text("OFFSET", 180u, y + 185u, white);
                draw_hex(results[i].offset, 420u, y + 185u, white);
                draw_text("SIZE", 180u, y + 245u, white);
                draw_hex(results[i].size, 330u, y + 245u, white);
            } else {
                draw_text("NOT FOUND IN TABLE", 180u, y + 90u, white);
            }
        }
    }
#endif
    snapshot_barrier();
}
#endif

static volatile const uint8_t *smem(void)
{
    return (volatile const uint8_t *)smem_vaddr;
}

static volatile uint32_t *lock_register(void)
{
    return (volatile uint32_t *)tcsr_lock_vaddr;
}

static uint16_t rd16(volatile const uint8_t *p, uint32_t offset)
{
#ifdef SMEM_PROBE_READ_CHECK
    SMEM_PROBE_READ_CHECK(offset, 2u);
#endif
    return (uint16_t)p[offset] | ((uint16_t)p[offset + 1u] << 8);
}

static uint32_t rd32(volatile const uint8_t *p, uint32_t offset)
{
#ifdef SMEM_PROBE_READ_CHECK
    SMEM_PROBE_READ_CHECK(offset, 4u);
#endif
    return (uint32_t)p[offset] | ((uint32_t)p[offset + 1u] << 8) |
           ((uint32_t)p[offset + 2u] << 16) |
           ((uint32_t)p[offset + 3u] << 24);
}

static void put_hex(uint32_t value)
{
    static const char digits[] = "0123456789ABCDEF";
    for (int shift = 28; shift >= 0; shift -= 4) {
        microkit_dbg_putc(digits[(value >> shift) & 0xfu]);
    }
}

static void put_dec(uint32_t value)
{
    char digits[10];
    unsigned n = 0;
    do {
        digits[n++] = (char)('0' + value % 10u);
        value /= 10u;
    } while (value != 0u && n < sizeof(digits));
    while (n != 0u) {
        microkit_dbg_putc(digits[--n]);
    }
}

static int range_ok(uint32_t offset, uint32_t size, uint32_t limit)
{
    return offset <= limit && size <= limit - offset;
}

static int scan_partition(uint32_t part_offset, uint32_t part_size,
                          uint32_t cacheline,
                          uint32_t requested_host0, uint32_t requested_host1,
                          uint32_t item_id, struct item_result *result)
{
    volatile const uint8_t *base = smem();
    uint32_t part_end_limit = SMEM_BYTES - SMEM_PTABLE_BYTES;
    if (part_size < SMEM_PARTITION_HEADER ||
        !range_ok(part_offset, part_size, part_end_limit)) {
        return -1;
    }
    if (base[part_offset] != '$' || base[part_offset + 1u] != 'P' ||
        base[part_offset + 2u] != 'R' || base[part_offset + 3u] != 'T') {
        return -1;
    }
    uint32_t host0 = rd16(base, part_offset + 4u);
    uint32_t host1 = rd16(base, part_offset + 6u);
    if (!((host0 == requested_host0 && host1 == requested_host1) ||
          (host0 == requested_host1 && host1 == requested_host0)) ||
        rd32(base, part_offset + 8u) != part_size) {
        return -1;
    }
    uint32_t uncached_end = rd32(base, part_offset + 12u);
    uint32_t cached_end = rd32(base, part_offset + 16u);
    if (uncached_end < SMEM_PARTITION_HEADER || uncached_end > part_size ||
        cached_end < uncached_end || cached_end > part_size) {
        return -1;
    }

    uint32_t cursor = SMEM_PARTITION_HEADER;
    for (uint32_t step = 0; cursor < uncached_end && step < SMEM_MAX_SCAN_STEPS; ++step) {
        if (uncached_end - cursor < SMEM_PRIVATE_ENTRY) {
            return -1;
        }
        uint32_t entry = part_offset + cursor;
        if (rd16(base, entry) != SMEM_CANARY) {
            return -1;
        }
        uint32_t found_id = rd16(base, entry + 2u);
        uint32_t stored_size = rd32(base, entry + 4u);
        uint32_t pad_data = rd16(base, entry + 8u);
        uint32_t pad_header = rd16(base, entry + 10u);
        if (stored_size == 0u || pad_data >= stored_size ||
            pad_header > part_size - cursor - SMEM_PRIVATE_ENTRY) {
            return -1;
        }
        uint32_t data_offset = cursor + SMEM_PRIVATE_ENTRY + pad_header;
        if (!range_ok(data_offset, stored_size, uncached_end)) {
            return -1;
        }
        if (found_id == item_id) {
            result->found = 1u;
            result->offset = part_offset + data_offset;
            result->size = stored_size - pad_data;
            result->host0 = host0;
            result->host1 = host1;
#ifdef SMEM_IPA_ITEM
            result->cached = 0u;
#endif
            return 1;
        }
        uint32_t next = data_offset + stored_size;
        if (next <= cursor) {
            return -1;
        }
        cursor = next;
    }
    if (cursor != uncached_end) {
        return -1;
    }

    /* qcom_smem_get() searches the cacheable allocator from the end of the
     * partition after the uncached list. Its entry header is cacheline
     * aligned and precedes the item bytes. */
    if (cached_end < part_size) {
        if (cacheline < SMEM_PRIVATE_ENTRY ||
            (cacheline & (cacheline - 1u)) != 0u) return -1;
        uint32_t partition_header =
            (SMEM_PARTITION_HEADER + cacheline - 1u) & ~(cacheline - 1u);
        uint32_t entry_header =
            (SMEM_PRIVATE_ENTRY + cacheline - 1u) & ~(cacheline - 1u);
        if (partition_header >= part_size) return -1;
        uint32_t entry = part_size - partition_header;
        for (uint32_t step = 0; entry > cached_end &&
             step < SMEM_MAX_SCAN_STEPS; ++step) {
            if (!range_ok(entry, SMEM_PRIVATE_ENTRY, part_size) ||
                entry < cached_end) return -1;
            if (rd16(base, part_offset + entry) != SMEM_CANARY) return -1;
            uint32_t found_id = rd16(base, part_offset + entry + 2u);
            uint32_t stored_size = rd32(base, part_offset + entry + 4u);
            uint32_t pad_data = rd16(base, part_offset + entry + 8u);
            if (stored_size == 0u || pad_data >= stored_size ||
                stored_size > entry - cached_end) return -1;
            uint32_t data_offset = entry - stored_size;
            if (!range_ok(data_offset, stored_size, entry)) return -1;
            if (found_id == item_id) {
                result->found = 1u;
                result->offset = part_offset + data_offset;
                result->size = stored_size - pad_data;
                result->host0 = host0;
                result->host1 = host1;
#ifdef SMEM_IPA_ITEM
                result->cached = 1u;
#endif
                return 1;
            }
            if (entry < stored_size + entry_header) return -1;
            uint32_t next = entry - stored_size - entry_header;
            if (next >= entry) return -1;
            entry = next;
        }
        if (entry > cached_end) return -1;
    }
    return 0;
}

static int find_items(struct item_result results[PROBE_ITEM_COUNT],
                      struct route_result *route)
{
    volatile const uint8_t *base = smem();
    if (rd32(base, SMEM_HEADER_INITIALIZED) != 1u ||
        rd32(base, SMEM_HEADER_RESERVED) != 0u) {
        return -1;
    }
    if ((rd32(base, SMEM_SBL_VERSION) >> 16) != 12u) {
        return -2;
    }
    uint32_t table = SMEM_BYTES - SMEM_PTABLE_BYTES;
    if (base[table] != '$' || base[table + 1u] != 'T' ||
        base[table + 2u] != 'O' || base[table + 3u] != 'C' ||
        rd32(base, table + 4u) != 1u) {
        return -3;
    }
    uint32_t count = rd32(base, table + 8u);
    if (count == 0u || count > SMEM_MAX_PARTITIONS ||
        !range_ok(SMEM_PTABLE_HEADER, count * SMEM_PTABLE_ENTRY,
                  SMEM_PTABLE_BYTES)) {
        return -4;
    }

    /* GLINK descriptor/FIFOs and modem/AP SMP2P items from Atoll DTS. */
#ifdef SMEM_IPA_ITEM
    const uint32_t ids[PROBE_ITEM_COUNT] = {497u};
#else
    const uint32_t ids[PROBE_ITEM_COUNT] = {478u, 479u, 480u, 435u, 428u};
#endif
    uint32_t private_part_offset = 0u, private_part_size = 0u;
    uint32_t private_cacheline = 0u, private_index = 0u;
    uint32_t global_part_offset = 0u, global_part_size = 0u;
    uint32_t global_cacheline = 0u, global_index = 0u;
    for (uint32_t i = 0; i < count; ++i) {
        uint32_t entry = table + SMEM_PTABLE_HEADER + i * SMEM_PTABLE_ENTRY;
        uint32_t offset = rd32(base, entry);
        uint32_t size = rd32(base, entry + 4u);
        uint32_t h0 = rd16(base, entry + 12u);
        uint32_t h1 = rd16(base, entry + 14u);
        if ((h0 == 0u && h1 == 1u) || (h0 == 1u && h1 == 0u)) {
            if (private_part_offset != 0u) return -5;
            private_part_offset = offset;
            private_part_size = size;
            private_cacheline = rd32(base, entry + 16u);
            private_index = i;
        }
        if (h0 == 0xfffeu && h1 == 0xfffeu) {
            if (global_part_offset != 0u) return -5;
            global_part_offset = offset;
            global_part_size = size;
            global_cacheline = rd32(base, entry + 16u);
            global_index = i;
        }
    }

    /* Match qcom_smem_get(host=1): use the 0:1 private partition when Linux
     * enumerated it; otherwise use the validated global private partition. */
    uint32_t part_offset, part_size, cacheline, part_index;
    if (private_part_offset != 0u) {
        part_offset = private_part_offset;
        part_size = private_part_size;
        cacheline = private_cacheline;
        part_index = private_index;
        route->global_fallback = 0u;
    } else if (global_part_offset != 0u) {
        part_offset = global_part_offset;
        part_size = global_part_size;
        cacheline = global_cacheline;
        part_index = global_index;
        route->global_fallback = 1u;
    } else {
        return -8;
    }
    route->partition_index = part_index;
    route->offset = part_offset;
    route->size = part_size;
    route->cacheline = cacheline;
    if (part_size < SMEM_PARTITION_HEADER ||
        !range_ok(part_offset, part_size, table)) return -9;
    if (base[part_offset] != '$' || base[part_offset + 1u] != 'P' ||
        base[part_offset + 2u] != 'R' || base[part_offset + 3u] != 'T' ||
        rd32(base, part_offset + 8u) != part_size) return -10;
    route->free_uncached = rd32(base, part_offset + 12u);
    route->free_cached = rd32(base, part_offset + 16u);

    for (uint32_t item_index = 0; item_index < PROBE_ITEM_COUNT; ++item_index) {
        int state = scan_partition(part_offset, part_size, cacheline,
                                   route->global_fallback ? 0xfffeu : 0u,
                                   route->global_fallback ? 0xfffeu : 1u,
                                   ids[item_index], &results[item_index]);
        if (state < 0) return -20 - (int)item_index;
        if (state == 1) results[item_index].partition = part_index;
    }
    return 0;
}

#ifdef SMEM_IPA_LAYOUT
#include "ipa-smem-layout.inc"
#elif defined(SMEM_IPA_ALLOC)
#include "ipa-smem-allocate.inc"
#else
void init(void)
{
    struct item_result items[PROBE_ITEM_COUNT] = {{0}};
    struct route_result route = {0};
    microkit_dbg_puts("SUNFISH SMEM DIRECTORY PROBE\r\n");
    volatile uint32_t *lock = lock_register();
    int acquired = 0;
    for (uint32_t i = 0; i < HWLOCK_TRIES; ++i) {
#ifdef SMEM_IPA_ITEM
        census_attempts = i + 1u;
#endif
        *lock = HWLOCK_APPS_OWNER;
        snapshot_barrier();
        if (*lock == HWLOCK_APPS_OWNER) {
            acquired = 1;
            break;
        }
    }
    if (!acquired) {
        microkit_dbg_puts("SMEM HWLOCK TIMEOUT\r\n");
        show_results(items, &route, -7);
        return;
    }

    snapshot_barrier();
    int result = find_items(items, &route);
#ifdef SMEM_IPA_ITEM
    census_scans = 1u;
    if (result == 0) {
        struct item_result second[PROBE_ITEM_COUNT] = {{0}};
        struct route_result second_route = {0};
        snapshot_barrier();
        int again = find_items(second, &second_route);
        census_scans = 2u;
        if (again != 0 || items[0].found != second[0].found ||
            items[0].offset != second[0].offset || items[0].size != second[0].size ||
            items[0].partition != second[0].partition || items[0].host0 != second[0].host0 ||
            items[0].host1 != second[0].host1 || items[0].cached != second[0].cached ||
            route.partition_index != second_route.partition_index || route.offset != second_route.offset ||
            route.size != second_route.size || route.cacheline != second_route.cacheline ||
            route.free_uncached != second_route.free_uncached || route.free_cached != second_route.free_cached ||
            route.global_fallback != second_route.global_fallback) result = -30;
        if (items[0].found && items[0].size != 0x2000u) result = -32;
    }
    snapshot_barrier();
    if (*lock != HWLOCK_APPS_OWNER) {
        result = -31; /* Never release a lock no longer owned by APPS. */
    } else {
        *lock = 0u;
        snapshot_barrier();
        census_release_sent = 1u;
    }
#else
    *lock = 0u;
    snapshot_barrier();
#endif
    if (result != 0) {
        microkit_dbg_puts("SMEM DIRECTORY INVALID ");
        put_dec((uint32_t)(-result));
        microkit_dbg_puts("\r\n");
        if (route.size != 0u) {
            microkit_dbg_puts(route.global_fallback ? "ROUTE GLOBAL " :
                              "ROUTE HOST 0:1 ");
            put_dec(route.partition_index);
            microkit_dbg_puts(" OFFSET ");
            put_hex(route.offset);
            microkit_dbg_puts(" SIZE ");
            put_hex(route.size);
            microkit_dbg_puts(" CACHELINE ");
            put_hex(route.cacheline);
            microkit_dbg_puts(" FREE ");
            put_hex(route.free_uncached);
            microkit_dbg_putc(' ');
            put_hex(route.free_cached);
            microkit_dbg_puts("\r\n");
        }
        show_results(items, &route, result);
        return;
    }
    for (uint32_t i = 0; i < PROBE_ITEM_COUNT; ++i) {
        microkit_dbg_puts("ITEM ");
#ifdef SMEM_IPA_ITEM
        put_dec(497u);
#else
        put_dec(i == 0u ? 478u : i == 1u ? 479u : i == 2u ? 480u :
                i == 3u ? 435u : 428u);
#endif
        if (!items[i].found) {
            microkit_dbg_puts(" NOT FOUND\r\n");
        } else {
            microkit_dbg_puts(" OFFSET ");
            put_hex(items[i].offset);
            microkit_dbg_puts(" SIZE ");
            put_hex(items[i].size);
            microkit_dbg_puts("\r\n");
        }
    }
    microkit_dbg_puts("SMEM DIRECTORY READ COMPLETE; GLINK UNTOUCHED\r\n");
    show_results(items, &route, 0);
}

void notified(microkit_channel channel)
{
    (void)channel;
}
#endif
