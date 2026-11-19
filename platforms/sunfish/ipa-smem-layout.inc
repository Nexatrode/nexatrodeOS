/* Metadata snapshot only. No allocation, SMEM stores or item payload access. */
struct layout_row {
    uint32_t offset, size, host0, host1, cacheline;
};
struct layout_snapshot {
    struct item_result item[PROBE_ITEM_COUNT];
    struct route_result route;
    uint32_t count, item_limit;
    struct layout_row rows[SMEM_MAX_PARTITIONS];
};
struct layout_guard {
    uint32_t mask, first_index, first_cause, partial_empty, invalid_range, overlaps;
    struct layout_row first;
};
static struct layout_snapshot layout_first, layout_second;

static uint32_t layout_lock_read(void)
{
#ifdef SMEM_PROBE_HOST_TEST
    return smem_layout_test_lock_read();
#else
    return *lock_register();
#endif
}

static void layout_lock_write(uint32_t value)
{
#ifdef SMEM_PROBE_HOST_TEST
    smem_layout_test_lock_write(value);
#else
    *lock_register() = value;
#endif
}

static int capture_layout(struct layout_snapshot *out)
{
    int result = find_items(out->item, &out->route);
    if (result) return result;
    volatile const uint8_t *base = smem();
    uint32_t table = SMEM_BYTES - SMEM_PTABLE_BYTES;
    out->count = rd32(base, table + 8u);
    if (!out->count || out->count > SMEM_MAX_PARTITIONS ||
        !range_ok(SMEM_PTABLE_HEADER, out->count * SMEM_PTABLE_ENTRY, SMEM_PTABLE_BYTES)) return -4;
    uint32_t info = SMEM_PTABLE_HEADER + out->count * SMEM_PTABLE_ENTRY;
    if (!range_ok(info, 20u, SMEM_PTABLE_BYTES)) return -42;
    out->item_limit = 512u;
    if (base[table + info] == 'S' && base[table + info + 1u] == 'I' &&
        base[table + info + 2u] == 'I' && base[table + info + 3u] == 'I') {
        out->item_limit = rd16(base, table + info + 16u);
    }
    for (uint32_t i = 0; i < out->count; ++i) {
        uint32_t entry = table + SMEM_PTABLE_HEADER + i * SMEM_PTABLE_ENTRY;
        out->rows[i] = (struct layout_row){rd32(base, entry), rd32(base, entry + 4u),
            rd16(base, entry + 12u), rd16(base, entry + 14u), rd32(base, entry + 16u)};
    }
    return 0;
}

static int layout_matches(const struct layout_snapshot *a, const struct layout_snapshot *b)
{
    const struct item_result *ai = a->item, *bi = b->item;
    const struct route_result *ar = &a->route, *br = &b->route;
    if (a->count != b->count || a->item_limit != b->item_limit ||
        ai->found != bi->found || ai->offset != bi->offset || ai->size != bi->size ||
        ai->partition != bi->partition || ai->host0 != bi->host0 || ai->host1 != bi->host1 || ai->cached != bi->cached ||
        ar->partition_index != br->partition_index || ar->offset != br->offset || ar->size != br->size ||
        ar->cacheline != br->cacheline || ar->free_uncached != br->free_uncached ||
        ar->free_cached != br->free_cached || ar->global_fallback != br->global_fallback) return 0;
    for (uint32_t i = 0; i < a->count; ++i) {
        const struct layout_row *x = &a->rows[i], *y = &b->rows[i];
        if (x->offset != y->offset || x->size != y->size || x->host0 != y->host0 ||
            x->host1 != y->host1 || x->cacheline != y->cacheline) return 0;
    }
    return 1;
}

static struct layout_guard classify_layout(const struct layout_snapshot *snapshot)
{
    struct layout_guard result = {0};
    result.first_index = 0xffffffffu;
    const struct route_result *route = &snapshot->route;
    if (route->offset < 8400u) result.mask |= 8u;
    for (uint32_t i = 0; i < snapshot->count; ++i) {
        if (i == route->partition_index) continue;
        const struct layout_row *row = &snapshot->rows[i];
        uint32_t cause = 0u;
        /* Preserve the stopped guard's exact classification, including its
         * stricter rejection of a slot with just one zero field. */
        if (!row->offset && !row->size) continue;
        if (!row->offset || !row->size) { cause = 1u; ++result.partial_empty; }
        else if (!range_ok(row->offset, row->size, SMEM_BYTES - SMEM_PTABLE_BYTES)) {
            cause = 2u; ++result.invalid_range;
        } else if (row->offset < route->offset + route->size && route->offset < row->offset + row->size) {
            cause = 4u; ++result.overlaps;
        }
        result.mask |= cause;
        if (cause && result.first_index == 0xffffffffu) {
            result.first_index = i; result.first_cause = cause; result.first = *row;
        }
    }
    return result;
}

static int read_layout_once(void)
{
    int acquired = 0;
    for (uint32_t i = 0; i < HWLOCK_TRIES; ++i) {
        census_attempts = i + 1u;
        layout_lock_write(HWLOCK_APPS_OWNER); snapshot_barrier();
        if (layout_lock_read() == HWLOCK_APPS_OWNER) { acquired = 1; break; }
    }
    if (!acquired) return -7;
    snapshot_barrier();
    int result = capture_layout(&layout_first);
    census_scans = 1u;
    if (!result) {
        snapshot_barrier();
        int second = capture_layout(&layout_second);
        census_scans = 2u;
        if (second || !layout_matches(&layout_first, &layout_second)) result = -30;
    }
    snapshot_barrier();
    if (layout_lock_read() != HWLOCK_APPS_OWNER) result = -31;
    else { layout_lock_write(0u); snapshot_barrier(); census_release_sent = 1u; }
    return result;
}

static void show_layout(int error)
{
    const uint32_t white = 0x00f4f7ffu, cyan = 0x0000e5e5u;
    struct layout_guard guard = {0}; guard.first_index = 0xffffffffu;
    /* Foreign geometry is evaluated from captured metadata only after release.
     * A scan failure never classifies a partial snapshot as a valid layout. */
    if (!error) guard = classify_layout(&layout_first);
    const struct route_result *route = &layout_first.route;
    fill_screen(0x00122b52u);
    draw_text("IPA SMEM TABLE CHECK", 80u, 100u, cyan);
    draw_text(error ? "SMEM TABLE STOP" : "SMEM TABLE MATCH", 80u, 200u, white);
    const char *labels[] = {"SCANS DONE", "LOCK ATTEMPTS", "RELEASE SENT", "TABLE COUNT", "ITEM LIMIT",
        "TARGET INDEX", "TARGET OFFSET", "TARGET BYTES", "FREE UNCACHED", "ITEM PRESENT", "GUARD MASK",
        "FIRST BAD ROW", "BAD CAUSE", "BAD OFFSET", "BAD BYTES", "BAD HOST 0", "BAD HOST 1",
        "PARTIAL EMPTY", "RANGE FAILS", "OVERLAPS", "LOCAL ERROR"};
    const uint32_t values[] = {census_scans, census_attempts, census_release_sent, layout_first.count, layout_first.item_limit,
        route->partition_index, route->offset, route->size, route->free_uncached, layout_first.item[0].found,
        guard.mask, guard.first_index, guard.first_cause, guard.first.offset, guard.first.size,
        guard.first.host0, guard.first.host1, guard.partial_empty, guard.invalid_range, guard.overlaps,
        (uint32_t)(error < 0 ? -error : error)};
    for (uint32_t i = 0; i < sizeof(values) / sizeof(values[0]); ++i) {
        draw_text(labels[i], 80u, 300u + i * 72u, i & 1u ? white : cyan);
        draw_hex(values[i], 750u, 300u + i * 72u, i & 1u ? white : cyan);
    }
    draw_text("LAYOUT ONLY NO ALLOCATION", 80u, 1830u, cyan);
    draw_text("1 EMPTY 2 RANGE 4 OVERLAP", 80u, 1910u, white);
    draw_text("NO SMEM WRITE OR PAYLOAD", 80u, 1990u, cyan);
    draw_text("NO IPA SMMU DMA OR PAS", 80u, 2070u, white);
    draw_text("RESTART WHEN DONE", 80u, 2150u, cyan);
    snapshot_barrier();
}

void init(void)
{
    microkit_dbg_puts("SMEM PARTITION TABLE READ ONLY\r\n");
    int result = read_layout_once();
    microkit_dbg_puts("RESULT "); put_dec((uint32_t)(result < 0 ? -result : result));
    microkit_dbg_puts(" SCANS "); put_hex(census_scans); microkit_dbg_puts("\r\n");
    show_layout(result);
}

void notified(microkit_channel channel) { (void)channel; }
