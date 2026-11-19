/*
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Microkit-side runtime boundary probe. This is not GHC's RTS; it validates
 * the entry-point, debug-output, and fixed-heap contract that a port needs.
 */
#include <microkit.h>
#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
#include <utime.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#include "mblock_arena.h"
#include "sel4_libc.h"

#define HEAP_SIZE (64u * 1024u)
#define HEAP_ALIGNMENT 16u
#define CAPABILITY_CHANNEL 0u
#define CAPABILITY_REQUEST 0x701u
#define CAPABILITY_REPLY 0x1701u
#define CAPABILITY_VALUE UINT64_C(0x53454c3447424348)

static _Alignas(HEAP_ALIGNMENT) uint8_t heap[HEAP_SIZE];
static size_t heap_used;

#ifdef GHC_OSMEM_INTEGRATION
#include "Rts.h"
#include "sm/OSMem.h"

void osMemInit(void);
void *osGetMBlocks(uint32_t count);
void osFreeMBlocks(void *addr, uint32_t count);
void osFreeAllMBlocks(void);

__attribute__((noreturn)) void barf(const char *message, ...)
{
    microkit_dbg_puts("GHCRTS|FAIL: GHC OSMem barf: ");
    microkit_dbg_puts(message);
    microkit_dbg_puts("\n");
    for (;;) {
        __asm__ volatile("wfe");
    }
}

static bool check_ghc_osmem_adapter(void)
{
    const uintptr_t mblock_size = 1u << 20;
    osMemInit();
    void *first = osGetMBlocks(2);
    void *next = osGetMBlocks(3);
    if (((uintptr_t)first % mblock_size) != 0 ||
        ((uintptr_t)next % mblock_size) != 0 ||
        (uintptr_t)next - (uintptr_t)first != 2 * mblock_size) {
        return false;
    }

    osFreeMBlocks(first, 2);
    if (osGetMBlocks(2) != first) {
        return false;
    }
    osFreeAllMBlocks();
    void *whole_arena = osGetMBlocks(8);
    if (whole_arena != first) {
        return false;
    }
    osFreeMBlocks(whole_arena, 8);

#if defined(USE_LARGE_ADDRESS_SPACE)
    W_ reserved_size = (W_)1 << 38;
    void *reserved = osReserveHeapMemory(NULL, &reserved_size);
    if (reserved != first || reserved_size != 8 * mblock_size) {
        return false;
    }
    osCommitMemory(reserved, reserved_size);
    osDecommitMemory((uint8_t *)reserved + mblock_size, mblock_size);
    osReleaseHeapMemory();
#endif

    return true;
}

#endif

static void print_number(uint32_t value)
{
    char digits[10];
    size_t length = 0;

    do {
        digits[length++] = (char)('0' + (value % 10));
        value /= 10;
    } while (value != 0 && length < sizeof(digits));

    while (length != 0) {
        microkit_dbg_putc(digits[--length]);
    }
}

static void *heap_alloc(size_t size)
{
    if (size == 0 || size > HEAP_SIZE - heap_used) {
        return NULL;
    }

    size_t aligned_used = (heap_used + HEAP_ALIGNMENT - 1) & ~(HEAP_ALIGNMENT - 1);
    if (aligned_used > HEAP_SIZE || size > HEAP_SIZE - aligned_used) {
        return NULL;
    }

    void *result = &heap[aligned_used];
    heap_used = aligned_used + size;
    return result;
}

static int compare_ints(const void *lhs, const void *rhs)
{
    int a = *(const int *)lhs;
    int b = *(const int *)rhs;
    return (a > b) - (a < b);
}

static bool check_sel4_libc(void)
{
    char *first = malloc(31);
    char *second = malloc(80);
    void *page_aligned = NULL;
    unsigned char *zeroed = calloc(19, 4);
    char *copy = strdup("bounded libc");
    int values[] = { 7, -3, 4, 4, 0, 12, -9 };
    qsort(values, sizeof(values) / sizeof(values[0]), sizeof(values[0]), compare_ints);
    if (first == NULL || second == NULL || zeroed == NULL || copy == NULL ||
        posix_memalign(&page_aligned, 4096, 64) != 0 ||
        ((uintptr_t)page_aligned % 4096) != 0 || ((uintptr_t)first % 16) != 0 ||
        strcmp(strerror(ENOSYS), "Function not implemented") != 0 ||
        strcmp(strerror(-1), "Unknown error") != 0 ||
        strcmp(copy, "bounded libc") != 0 || strlen(copy) != 12 ||
        memchr(copy, 'l', 12) != copy + 8 || strnlen(copy, 5) != 5 ||
        strcspn(copy, " c") != 7 ||
        strncmp(copy, "bounded", 7) != 0 || strrchr(copy, 'c') != copy + 11) {
        return false;
    }
    for (size_t i = 1; i < sizeof(values) / sizeof(values[0]); ++i) {
        if (values[i - 1] > values[i]) return false;
    }
    for (size_t i = 0; i < 19 * 4; ++i) {
        if (zeroed[i] != 0) return false;
    }
    first[0] = 'a';
    first[1] = 'b';
    first[2] = 'c';
    (void)memmove(first + 1, first, 3);
    if (first[0] != 'a' || first[1] != 'a' || first[2] != 'b' || first[3] != 'c' ||
        memcmp(first, "aabc", 4) != 0) {
        return false;
    }
    second[0] = 'r';
    second[1] = 't';
    second = realloc(second, 160);
    if (second == NULL || second[0] != 'r' || second[1] != 't') return false;
    (void)memset(second + 2, 'x', 10);
    char bounded[8];
    (void)strncpy(bounded, "ok", sizeof(bounded));
    if (bounded[0] != 'o' || bounded[1] != 'k' || bounded[2] != '\0' ||
        memcmp(second + 2, "xxxxxxxxxx", 10) != 0) {
        return false;
    }

    struct stat standard_output;
    errno = 0;
    if (getenv("PATH") != NULL || write(STDOUT_FILENO, "GHCRTS|POSIX_WRITE_OK\n", 22) != 22 ||
        fstat(STDOUT_FILENO, &standard_output) != 0 ||
        !S_ISFIFO(standard_output.st_mode) || isatty(STDOUT_FILENO) != 0 || errno != ENOTTY) {
        return false;
    }
    errno = 0;
    if (read(STDIN_FILENO, bounded, sizeof(bounded)) != -1 || errno != ENOSYS) {
        return false;
    }
    errno = 0;
    if (close(STDOUT_FILENO) != -1 || errno != ENOSYS || close(9) != -1 || errno != EBADF) {
        return false;
    }
    errno = 0;
    if (open("/not-mounted", O_RDONLY) != -1 || errno != ENOSYS) {
        return false;
    }
    errno = 0;
    if (link("/missing-a", "/missing-b") != -1 || errno != ENOSYS) {
        return false;
    }
    mode_t old_mask = umask(0027);
    if (old_mask != 0022 || umask(0077) != 0027 || umask(old_mask) != 0077) {
        return false;
    }
    errno = 0;
    if (utime("/not-mounted", NULL) != -1 || errno != ENOSYS) {
        return false;
    }

    char formatted[32];
    if (snprintf(formatted, sizeof(formatted), "X:%s:%d:%#x", "ok", -12, 42) != 13 ||
        strcmp(formatted, "X:ok:-12:0x2a") != 0) {
        return false;
    }
    if (snprintf(formatted, sizeof(formatted), "%hhd:%hhu:%04ld:%.3s",
                 255, 0x123, -7L, "hello") != 14 ||
        strcmp(formatted, "-1:35:-007:hel") != 0) {
        return false;
    }
    char truncated[5];
    if (snprintf(truncated, sizeof(truncated), "%s", "abcdef") != 6 ||
        strcmp(truncated, "abcd") != 0) {
        return false;
    }
    if (fprintf(stderr, "GHCRTS|STDIO_FORMAT_OK:%s:%u\n", "yes", 7u) < 0 ||
        fflush(stderr) != 0) {
        return false;
    }
    errno = 0;
    if (fopen("/not-mounted", "r") != NULL || errno != ENOSYS) return false;
    errno = 0;
    if (getc(stdin) != EOF || errno != ENOSYS) return false;

    free(copy);
    free(zeroed);
    free(page_aligned);
    free(first);
    free(second);

    /* Coalescing should return the large fixed arena to one usable run. */
    void *large = malloc(900u * 1024u);
    if (large == NULL) return false;
    free(large);
    return true;
}

void init(void)
{
    if (!check_sel4_libc()) {
        microkit_dbg_puts("GHCRTS|FAIL: bounded freestanding libc\n");
        return;
    }
    microkit_dbg_puts("GHCRTS|SEL4_LIBC_OK: memory/string operations, aligned allocation, realloc, coalescing\n");

#ifdef GHC_OSMEM_INTEGRATION
    if (!check_ghc_osmem_adapter()) {
        microkit_dbg_puts("GHCRTS|FAIL: GHC OSMem adapter behavior\n");
        return;
    }
    microkit_dbg_puts("GHCRTS|GHC_OSMEM_ADAPTER_OK: aligned allocation, contiguous runs, reuse, bounded heap reserve/commit\n");
#endif

    ghc_mblock_init();
    void *two_blocks = ghc_mblock_alloc(2);
    void *three_blocks = ghc_mblock_alloc(3);
    if (two_blocks == NULL || three_blocks == NULL ||
        ((uintptr_t)two_blocks % GHC_MBLOCK_SIZE) != 0 ||
        ((uintptr_t)three_blocks % GHC_MBLOCK_SIZE) != 0 ||
        ghc_mblock_used() != 5 || ghc_mblock_alloc(4) != NULL ||
        ghc_mblock_free((uint8_t *)two_blocks + 1, 1) ||
        !ghc_mblock_free(two_blocks, 2)) {
        microkit_dbg_puts("GHCRTS|FAIL: megablock allocation/free\n");
        return;
    }

    void *reused_blocks = ghc_mblock_alloc(2);
    if (reused_blocks != two_blocks || ghc_mblock_used() != 5 ||
        !ghc_mblock_free(reused_blocks, 2) ||
        ghc_mblock_free(reused_blocks, 2) || ghc_mblock_used() != 3 ||
        !ghc_mblock_free(three_blocks, 3) || ghc_mblock_used() != 0 ||
        ghc_mblock_alloc(GHC_MBLOCK_COUNT + 1) != NULL) {
        microkit_dbg_puts("GHCRTS|FAIL: megablock reuse/bounds\n");
        return;
    }

    uint8_t *first = heap_alloc(37);
    uint64_t *aligned = heap_alloc(sizeof(*aligned));

    if (first == NULL || aligned == NULL || ((uintptr_t)aligned % _Alignof(uint64_t)) != 0) {
        microkit_dbg_puts("GHCRTS|FAIL: fixed heap allocation/alignment\n");
        return;
    }
    if (heap_alloc(0) != NULL || heap_alloc(HEAP_SIZE) != NULL) {
        microkit_dbg_puts("GHCRTS|FAIL: fixed heap bounds\n");
        return;
    }

    first[0] = 0x5a;
    *aligned = CAPABILITY_VALUE;
    if (first[0] != 0x5a || *aligned != CAPABILITY_VALUE) {
        microkit_dbg_puts("GHCRTS|FAIL: fixed heap read/write\n");
        return;
    }

    microkit_dbg_puts("GHCRTS|MBLOCK_ARENA_OK: aligned 1 MiB blocks; free/reuse/bounds OK\n");

    microkit_msginfo reply = microkit_ppcall(
        CAPABILITY_CHANNEL, microkit_msginfo_new(CAPABILITY_REQUEST, 0));
    if (microkit_msginfo_get_label(reply) != CAPABILITY_REPLY ||
        microkit_msginfo_get_count(reply) != 1 || microkit_mr_get(0) != CAPABILITY_VALUE) {
        microkit_dbg_puts("GHCRTS|FAIL: capability call\n");
        return;
    }

    microkit_dbg_puts("GHCRTS|READY: Microkit callbacks and fixed heap; used=");
    print_number((uint32_t)heap_used);
    microkit_dbg_puts(" bytes of ");
    print_number(HEAP_SIZE);
    microkit_dbg_puts(" bytes; capability call OK\n");
}

void notified(microkit_channel channel)
{
    (void)channel;
    microkit_dbg_puts("GHCRTS|FAIL: unexpected notification\n");
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo msginfo)
{
    (void)channel;
    (void)msginfo;
    microkit_dbg_puts("GHCRTS|FAIL: unexpected protected call\n");
    return seL4_MessageInfo_new(0, 0, 0, 0);
}
