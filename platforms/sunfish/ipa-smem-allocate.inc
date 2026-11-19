/*
 * Source: pinned qcom_smem_alloc_private()/qcom_smem_get_private().
 * This deliberately accepts only the empty 0:1 partition measured in IMG_9126.
 * It is an allocation/get experiment, not a general allocator or DMA lease.
 * Only the partition's first page is writable. Never touch item payloads,
 * reset a foreign lock, roll back a partial publication, or repeat an attempt.
 */
#define ALLOC_PART_INDEX 7u
#define ALLOC_PART_OFFSET 0x11d000u
#define ALLOC_PART_BYTES 0x7c000u
#define ALLOC_CACHELINE 0x20u
#define ALLOC_FREE_BEFORE 0x20u
#define ALLOC_ITEM_ID 497u
#define ALLOC_ITEM_BYTES 0x2000u
#define ALLOC_HEADER_OFFSET (ALLOC_PART_OFFSET + ALLOC_FREE_BEFORE)
#define ALLOC_FREE_AFTER (ALLOC_FREE_BEFORE + SMEM_PRIVATE_ENTRY + ALLOC_ITEM_BYTES)

uintptr_t smem_metadata_vaddr;
static uint32_t alloc_consumed;
static uint32_t alloc_pre_scans;
static uint32_t alloc_post_scans;
static uint32_t alloc_submitted;
static uint32_t alloc_completed;
static uint32_t alloc_published;
static uint32_t alloc_item_count;

#ifdef SMEM_IPA_ALLOC_V2
#include "ipa-smem-allocate-v2-guard.inc"
#endif

static uint32_t alloc_lock_read(void)
{
#ifdef SMEM_PROBE_HOST_TEST
    return smem_alloc_test_lock_read();
#else
    return *lock_register();
#endif
}

static void alloc_lock_write(uint32_t value)
{
#ifdef SMEM_PROBE_HOST_TEST
    smem_alloc_test_lock_write(value);
#else
    *lock_register() = value;
#endif
}

static int alloc_owned(void)
{
    snapshot_barrier();
    return alloc_lock_read() == HWLOCK_APPS_OWNER;
}

static int alloc_same(const struct item_result *a, const struct route_result *ar,
                      const struct item_result *b, const struct route_result *br)
{
    return a->found == b->found && a->offset == b->offset && a->size == b->size &&
           a->partition == b->partition && a->host0 == b->host0 && a->host1 == b->host1 &&
           a->cached == b->cached && ar->partition_index == br->partition_index &&
           ar->offset == br->offset && ar->size == br->size && ar->cacheline == br->cacheline &&
           ar->free_uncached == br->free_uncached && ar->free_cached == br->free_cached &&
           ar->global_fallback == br->global_fallback;
}

static int alloc_target(const struct item_result *item, const struct route_result *route)
{
    if (smem_metadata_vaddr != smem_vaddr + ALLOC_PART_OFFSET ||
        (smem_metadata_vaddr & 0xfffu) != 0u || item->found ||
        route->global_fallback || route->partition_index != ALLOC_PART_INDEX ||
        route->offset != ALLOC_PART_OFFSET || route->size != ALLOC_PART_BYTES ||
        route->cacheline != ALLOC_CACHELINE || route->free_uncached != ALLOC_FREE_BEFORE ||
        route->free_cached != ALLOC_PART_BYTES || ALLOC_FREE_AFTER > route->free_cached) return -41;

    volatile const uint8_t *base = smem();
    uint32_t table = SMEM_BYTES - SMEM_PTABLE_BYTES;
    uint32_t count = rd32(base, table + 8u);
    /* find_items already validated count/geometry; require room for optional
     * smem_info too. Linux uses 512 when the trailing SIII magic is absent. */
    uint32_t info = SMEM_PTABLE_HEADER + count * SMEM_PTABLE_ENTRY;
    if (!range_ok(info, 20u, SMEM_PTABLE_BYTES)) return -42;
    alloc_item_count = 512u;
    if (base[table + info] == 'S' && base[table + info + 1u] == 'I' &&
        base[table + info + 2u] == 'I' && base[table + info + 3u] == 'I') {
        alloc_item_count = rd16(base, table + info + 16u);
    }
    if (ALLOC_ITEM_ID < 8u || ALLOC_ITEM_ID >= alloc_item_count) return -42;

#ifdef SMEM_IPA_ALLOC_V2
    return alloc_foreign_v2(route, count);
#else
    /* A writable selected partition must not alias any other table entry,
     * the fixed global header or the TOC. Inspect metadata only. */
    if (ALLOC_PART_OFFSET < 8400u) return -43;
    for (uint32_t i = 0; i < count; ++i) {
        if (i == ALLOC_PART_INDEX) continue;
        uint32_t entry = table + SMEM_PTABLE_HEADER + i * SMEM_PTABLE_ENTRY;
        uint32_t offset = rd32(base, entry), size = rd32(base, entry + 4u);
        if (!offset && !size) continue;
        if (!offset || !size || !range_ok(offset, size, table)) return -43;
        if (offset < ALLOC_PART_OFFSET + ALLOC_PART_BYTES && ALLOC_PART_OFFSET < offset + size) return -43;
    }
    return 0;
#endif
}

static int alloc_write(uint32_t offset, uint32_t value, uint32_t bit)
{
    if (!alloc_owned()) return -31;
    const uint32_t offsets[] = {ALLOC_HEADER_OFFSET, ALLOC_HEADER_OFFSET + 4u,
        ALLOC_HEADER_OFFSET + 8u, ALLOC_PART_OFFSET + 12u};
    const uint32_t values[] = {(ALLOC_ITEM_ID << 16) | SMEM_CANARY,
        ALLOC_ITEM_BYTES, 0u, ALLOC_FREE_AFTER};
    uint32_t step = alloc_completed == 0u ? 0u : alloc_completed == 1u ? 1u :
        alloc_completed == 3u ? 2u : alloc_completed == 7u ? 3u : 4u;
    if (step == 4u || alloc_submitted != alloc_completed || bit != (1u << step) ||
        offset != offsets[step] || value != values[step]) return -44;
    alloc_submitted |= bit; /* Consume before a possibly uncertain write. */
#ifdef SMEM_PROBE_HOST_TEST
    smem_alloc_test_write(offset, value);
#else
    volatile uint32_t *word = (volatile uint32_t *)(smem_metadata_vaddr + offset - ALLOC_PART_OFFSET);
    *word = value;
#endif
    snapshot_barrier();
    if (!alloc_owned()) return -31;
    if (rd32(smem(), offset) != value) return -45;
    alloc_completed |= bit;
    return 0;
}

static int allocate_once(struct item_result *item, struct route_result *route)
{
    if (alloc_consumed) return -40;
    alloc_consumed = 1u;
    int acquired = 0, result = -7;
    for (uint32_t i = 0; i < HWLOCK_TRIES; ++i) {
        census_attempts = i + 1u;
        alloc_lock_write(HWLOCK_APPS_OWNER);
        if (alloc_owned()) { acquired = 1; break; }
    }
    if (!acquired) return result; /* Never release a foreign lock. */

    result = find_items(item, route);
    alloc_pre_scans = 1u;
    ++census_scans;
    if (result) goto release;
    if (!alloc_owned()) { result = -31; goto release; }
    struct item_result second[PROBE_ITEM_COUNT] = {{0}};
    struct route_result second_route = {0};
    result = find_items(second, &second_route);
    alloc_pre_scans = 2u;
    ++census_scans;
    if (result || !alloc_same(item, route, second, &second_route)) { result = -30; goto release; }
    result = alloc_target(item, route);
    if (result) goto release;

    /* Three header words, leaving the source ABI's reserved word untouched.
     * The item is invisible until the fourth write publishes the free offset.
     * alloc_write supplies a full barrier plus readback/owner checks each time.
     * The final explicit barrier is the pinned Linux wmb-before-publication. */
    result = alloc_write(ALLOC_HEADER_OFFSET, (ALLOC_ITEM_ID << 16) | SMEM_CANARY, 1u);
    if (result) goto release;
    result = alloc_write(ALLOC_HEADER_OFFSET + 4u, ALLOC_ITEM_BYTES, 2u);
    if (result) goto release;
    result = alloc_write(ALLOC_HEADER_OFFSET + 8u, 0u, 4u);
    if (result) goto release;
    snapshot_barrier();
    result = alloc_write(ALLOC_PART_OFFSET + 12u, ALLOC_FREE_AFTER, 8u);
    if (result) goto release;
    alloc_published = 1u;

    *item = (struct item_result){0}; *route = (struct route_result){0};
    result = find_items(item, route);
    alloc_post_scans = 1u;
    ++census_scans;
    if (result || !item->found || item->cached || item->offset != ALLOC_HEADER_OFFSET + SMEM_PRIVATE_ENTRY ||
        item->size != ALLOC_ITEM_BYTES || route->partition_index != ALLOC_PART_INDEX ||
        route->offset != ALLOC_PART_OFFSET || route->size != ALLOC_PART_BYTES ||
        route->cacheline != ALLOC_CACHELINE || route->free_uncached != ALLOC_FREE_AFTER ||
        route->free_cached != ALLOC_PART_BYTES || route->global_fallback) { result = -46; goto release; }
    if (!alloc_owned()) { result = -31; goto release; }
    second[0] = (struct item_result){0}; second_route = (struct route_result){0};
    result = find_items(second, &second_route);
    alloc_post_scans = 2u;
    ++census_scans;
    if (result || !alloc_same(item, route, second, &second_route)) result = -30;

release:
    if (!alloc_owned()) result = -31;
    else {
        alloc_lock_write(0u); snapshot_barrier(); census_release_sent = 1u;
    }
    return result;
}

static void show_allocation(const struct item_result *item, const struct route_result *route, int error)
{
    const uint32_t white = 0x00f4f7ffu, cyan = 0x0000e5e5u;
    uint32_t pa = item->found ? 0x86000000u + item->offset : 0u;
    uint32_t first = pa & ~0xfffu;
    uint32_t pages = item->found ? ((pa - first + item->size + 0xfffu) >> 12) : 0u;
    fill_screen(0x00122b52u);
#ifdef SMEM_IPA_ALLOC_V2
    draw_text("IPA SMEM ALLOC V2", 80u, 100u, cyan);
#else
    draw_text("IPA SMEM ITEM 497", 80u, 100u, cyan);
#endif
    draw_text(error ? "SMEM ALLOC GET STOP" : "SMEM ALLOC GET PASS", 80u, 200u, white);
    const char *labels[] = {"PRE SCANS", "POST SCANS", "LOCK ATTEMPTS", "RELEASE SENT",
        "WRITES SENT", "WRITES DONE", "PUBLISH VERIFIED", "PART INDEX", "PART OFFSET",
        "FREE BEFORE", "FREE AFTER", "ITEM PRESENT", "ITEM PA", "ITEM BYTES",
        "MAP CANDIDATE", "MAP PAGES", "ITEM LIMIT", "LOCAL ERROR", "SCANS DONE"};
    const uint32_t values[] = {alloc_pre_scans, alloc_post_scans, census_attempts, census_release_sent,
        alloc_submitted, alloc_completed, alloc_published, route->partition_index, route->offset,
        ALLOC_FREE_BEFORE, route->free_uncached, item->found, pa, item->size, first, pages,
        alloc_item_count, (uint32_t)(error < 0 ? -error : error), census_scans};
    for (uint32_t i = 0; i < sizeof(values) / sizeof(values[0]); ++i) {
        draw_text(labels[i], 80u, 320u + i * 80u, i & 1u ? white : cyan);
        draw_hex(values[i], 750u, 320u + i * 80u, i & 1u ? white : cyan);
    }
    draw_text("NO ITEM PAYLOAD ACCESS", 80u, 1830u, cyan);
    draw_text("NO IPA SMMU DMA OR PAS", 80u, 1910u, white);
    draw_text("ALLOCATION UNTIL REBOOT", 80u, 1990u, cyan);
    draw_text("NO RETRY OR FLASH", 80u, 2070u, white);
    draw_text("RESTART WHEN DONE", 80u, 2150u, cyan);
    snapshot_barrier();
}

void init(void)
{
    struct item_result item[PROBE_ITEM_COUNT] = {{0}};
    struct route_result route = {0};
    fill_screen(0x00122b52u);
    draw_text("SMEM ALLOCATION START", 80u, 200u, 0x00f4f7ffu);
    snapshot_barrier();
    int result = allocate_once(item, &route);
    microkit_dbg_puts("SMEM ALLOCATION RESULT "); put_dec((uint32_t)(result < 0 ? -result : result));
    microkit_dbg_puts(" ITEM OFFSET "); put_hex(item[0].offset); microkit_dbg_puts("\r\n");
    show_allocation(item, &route, result);
}

void notified(microkit_channel channel) { (void)channel; }
