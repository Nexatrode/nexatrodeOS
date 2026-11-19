#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static const volatile uint8_t *src;
static volatile uint8_t *dst;
static size_t length, prefix, payload, reads, writes, fault_read, fault_byte;
static unsigned checking, phase, events;
static size_t last_progress[5];
static size_t src_reads[128], dst_reads[128], dst_writes[128];
static char screen[25];
void loader_checkpoint(const char *text)
{
    assert(strlen(text) <= 24);
    strcpy(screen, text);
}
static uint8_t read8(const volatile uint8_t *base, size_t offset)
{
    assert(base == src || base == dst);
    assert(offset < length);
    if (checking) assert(offset < prefix || offset >= payload);
    else assert(base == src);
    (base == src ? src_reads : dst_reads)[offset]++;
    uint8_t value = base[offset];
    reads++;
    return reads == fault_read ? value ^ 1 : value;
}
static uint64_t read64(const volatile uint8_t *base, size_t offset)
{
    assert(!(((uintptr_t)base + offset) & 7));
    assert(offset + 8 <= length);
    uint64_t value = 0;
    for (unsigned n = 0; n < 8; n++) value |= (uint64_t)read8(base, offset + n) << (n * 8);
    return value;
}
static uint32_t read32(const volatile uint8_t *base, size_t offset)
{
    assert(!(((uintptr_t)base + offset) & 3) && offset + 4 <= length);
    uint32_t value = 0;
    for (unsigned n = 0; n < 4; n++) value |= (uint32_t)read8(base, offset + n) << (n * 8);
    return value;
}
static void write8(volatile uint8_t *base, size_t offset, uint8_t value)
{
    assert(!checking && base == dst && offset < length);
    assert(phase >= 2); /* The first-write marker precedes the first store. */
    dst_writes[offset]++;
    base[offset] = value;
    writes++;
}
static void write64(volatile uint8_t *base, size_t offset, uint64_t value)
{
    assert(!(((uintptr_t)base + offset) & 7));
    assert(offset + 8 <= length);
    for (unsigned n = 0; n < 8; n++) write8(base, offset + n, (uint8_t)(value >> (n * 8)));
}
static void write32(volatile uint8_t *base, size_t offset, uint32_t value)
{
    assert(!(((uintptr_t)base + offset) & 3) && offset + 4 <= length);
    for (unsigned n = 0; n < 4; n++) write8(base, offset + n, (uint8_t)(value >> (n * 8)));
}
static void progress(unsigned stage, size_t offset)
{
    assert(offset <= length);
    events++;
    assert(stage < 5 && offset >= last_progress[stage]);
    last_progress[stage] = offset;
    if (stage == 0) { assert(!checking && !reads && !writes); phase = 1; }
    if (stage == 1) { assert(!checking && reads && !writes && phase == 1); phase = 2; }
    if (stage == 2) assert(!checking && writes == offset);
    if (stage >= 3) assert(checking && !writes);
}
#define LOADER_RAM_READ8(base, offset) read8(base, offset)
#define LOADER_RAM_READ64(base, offset) read64(base, offset)
#define LOADER_RAM_READ32(base, offset) read32(base, offset)
#define LOADER_RAM_WRITE8(base, offset, value) write8(base, offset, value)
#define LOADER_RAM_WRITE64(base, offset, value) write64(base, offset, value)
#define LOADER_RAM_WRITE32(base, offset, value) write32(base, offset, value)
#define LOADER_RAM_PROGRESS(stage, offset) progress(stage, offset)
#define LOADER_RAM_CHUNK ((size_t)32)
#define SUNFISH_LOADER_COPY_PROGRESS 1
#include "../platforms/sunfish/loader-copy-progress.h"
_Noreturn void loader_copy_hold_stop(void) { abort(); }

static void reset_counts(unsigned verify)
{
    reads = writes = fault_read = events = phase = 0;
    checking = verify;
    memset(src_reads, 0, sizeof(src_reads));
    memset(dst_reads, 0, sizeof(dst_reads));
    memset(dst_writes, 0, sizeof(dst_writes));
    memset(last_progress, 0, sizeof(last_progress));
}

int main(void)
{
    _Alignas(8) uint8_t source[160], destination[160], snapshot[160];
    /* All relative alignments, byte tails, and both immutable boundaries.
     * Hooks check every actual access; canaries cover the entire allocation. */
    for (unsigned a = 0; a < 8; a++) for (unsigned b = 0; b < 8; b++)
        for (length = 48; length <= 79; length++) {
            src = source + 16 + a; dst = destination + 16 + b;
            prefix = 13; payload = 31;
            for (unsigned n = 0; n < sizeof(source); n++) source[n] = (uint8_t)(n * 17);
            memset(destination, 0xa5, sizeof(destination));
            memcpy(snapshot, destination, sizeof(destination));
            reset_counts(0);
            assert(loader_ram_copy(src, dst, prefix, payload, length) == LOADER_RAM_OK);
            assert(reads == length && writes == length && events >= 3);
            for (size_t n = 0; n < length; n++) {
                assert(src_reads[n] == 1 && dst_writes[n] == 1 && !dst_reads[n]);
                snapshot[16 + b + n] = source[16 + a + n];
            }
            assert(memcmp(snapshot, destination, sizeof(snapshot)) == 0);
            reset_counts(1);
            assert(loader_ram_verify(src, dst, prefix, payload, length) == LOADER_RAM_OK);
            assert(reads == 4 * (prefix + length - payload) && !writes);
            for (size_t n = 0; n < length; n++) {
                size_t expected = n < prefix || n >= payload ? 2 : 0;
                assert(src_reads[n] == expected && dst_reads[n] == expected);
            }
            assert(memcmp(snapshot, destination, sizeof(snapshot)) == 0);
            for (unsigned n = 0; n < sizeof(source); n++) assert(source[n] == (uint8_t)(n * 17));
        }
    src = source + 16; dst = destination + 16;
    length = 96; prefix = 16; payload = 32;
    memcpy(destination, source, sizeof(source));
    memcpy(snapshot, destination, sizeof(snapshot));
    dst[20] ^= 1; /* Mutable BSS is excluded. */
    reset_counts(1);
    assert(loader_ram_verify(src, dst, prefix, payload, length) == LOADER_RAM_OK);
    dst[20] ^= 1;
    /* Every byte within each word, and both immutable ranges, reports the
     * exact first offset without repair or a second read of the mismatch. */
    for (fault_byte = 0; fault_byte < length; fault_byte++) {
        if (fault_byte >= prefix && fault_byte < payload) continue;
        dst[fault_byte] ^= 1;
        reset_counts(1);
        assert(loader_ram_verify(src, dst, prefix, payload, length) == fault_byte);
        assert(src_reads[fault_byte] == 1 && dst_reads[fault_byte] == 1 && !writes);
        dst[fault_byte] ^= 1;
    }
    reset_counts(1);
    fault_read = 2 * (prefix + length - payload) + 6; /* First word of pass 2. */
    assert(loader_ram_verify(src, dst, prefix, payload, length) == 5);
    assert(memcmp(snapshot, destination, sizeof(snapshot)) == 0);
    src = source + 20; /* Relative alignment 4 exercises the 32-bit path. */
    memcpy(destination + 16, source + 20, length);
    for (fault_byte = 0; fault_byte < length; fault_byte++) {
        if (fault_byte >= prefix && fault_byte < payload) continue;
        dst[fault_byte] ^= 1;
        reset_counts(1);
        assert(loader_ram_verify(src, dst, prefix, payload, length) == fault_byte);
        assert(src_reads[fault_byte] == 1 && dst_reads[fault_byte] == 1 && !writes);
        dst[fault_byte] ^= 1;
    }
    reset_counts(0);
    assert(loader_ram_copy((const volatile uint8_t *)0xb0081000,
                           (volatile uint8_t *)0xb0000000, 0x7000, 0xf010, 0x46523c4) == LOADER_RAM_BOUNDS);
    assert(loader_ram_bounds((const volatile uint8_t *)0xb0081000,
                             (const volatile uint8_t *)0xc0000000, 0x7000, 0xf010, 0x46523c4));
    assert(loader_ram_copy(src, dst, 0, payload, length) == LOADER_RAM_BOUNDS);
    assert(loader_ram_copy(src, dst, payload + 1, payload, length) == LOADER_RAM_BOUNDS);
    assert(loader_ram_copy(src, dst, prefix, length + 1, length) == LOADER_RAM_BOUNDS);
    assert(loader_ram_copy(src, dst, prefix, payload, 0x5000001) == LOADER_RAM_BOUNDS);
    assert(loader_ram_copy(src, (volatile uint8_t *)src, prefix, payload, length) == LOADER_RAM_BOUNDS);
    assert(loader_ram_copy(src, (volatile uint8_t *)src + 1, prefix, payload, length) == LOADER_RAM_BOUNDS);
    assert(loader_ram_copy(0, dst, prefix, payload, length) == LOADER_RAM_BOUNDS);
    assert(loader_ram_copy(src, 0, prefix, payload, length) == LOADER_RAM_BOUNDS);
    assert(loader_ram_copy((const volatile uint8_t *)(UINTPTR_MAX - 2), dst,
                           prefix, payload, length) == LOADER_RAM_BOUNDS);
    assert(loader_ram_verify(src, src, prefix, payload, length) == LOADER_RAM_BOUNDS);
    assert(!reads && !writes && !events);
    loader_ram_mark(LOADER_RAM_READ, 0); assert(!strcmp(screen, "IMAGE COPY READ"));
    loader_ram_mark(LOADER_RAM_WRITE, 0); assert(!strcmp(screen, "IMAGE COPY WRITE"));
    loader_ram_mark(LOADER_RAM_COPY, 0x46513c4); assert(!strcmp(screen, "IMAGE COPY 046513C4"));
    loader_ram_mark(LOADER_RAM_VERIFY1, 0x12345678); assert(!strcmp(screen, "IMAGE VERIFY1 12345678"));
    loader_ram_mark(LOADER_RAM_VERIFY2, 0xabcdef); assert(!strcmp(screen, "IMAGE VERIFY2 00ABCDEF"));
}
