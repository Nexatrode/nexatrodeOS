/*
 * IMG_9132 identifies row 0 as the exact TOC-page descriptor, hosts ffff:ffff.
 * Linux maps that final 4-KiB page separately and does not enumerate these
 * hosts as an APPS or global data partition. Recognize only that exact row;
 * never relax the general data-partition range or overlap checks.
 */
#define ALLOC_EXPECTED_TABLE_COUNT 11u
#define ALLOC_TOC_ROW 0u
#define ALLOC_TOC_HOST 0xffffu

static int alloc_foreign_v2(const struct route_result *route, uint32_t count)
{
    volatile const uint8_t *base = smem();
    uint32_t table = SMEM_BYTES - SMEM_PTABLE_BYTES;
    if (count != ALLOC_EXPECTED_TABLE_COUNT) return -47;
    uint32_t toc = table + SMEM_PTABLE_HEADER + ALLOC_TOC_ROW * SMEM_PTABLE_ENTRY;
    if (rd32(base, toc) != table || rd32(base, toc + 4u) != SMEM_PTABLE_BYTES ||
        rd16(base, toc + 12u) != ALLOC_TOC_HOST || rd16(base, toc + 14u) != ALLOC_TOC_HOST) return -47;
    if (route->offset < 8400u || !range_ok(route->offset, route->size, table)) return -43;
    for (uint32_t i = 0; i < count; ++i) {
        if (i == route->partition_index || i == ALLOC_TOC_ROW) continue;
        uint32_t entry = table + SMEM_PTABLE_HEADER + i * SMEM_PTABLE_ENTRY;
        uint32_t offset = rd32(base, entry), size = rd32(base, entry + 4u);
        /* qcom_smem_enumerate_partitions() skips either zero field. */
        if (!offset || !size) continue;
        if (!range_ok(offset, size, table)) return -43;
        if (offset < route->offset + route->size && route->offset < offset + size) return -43;
    }
    return 0;
}
