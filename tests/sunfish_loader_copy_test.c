#include <assert.h>
#include <stdint.h>
#include <string.h>
static unsigned reads, fault_at;
static uint8_t test_read(const volatile uint8_t *base, size_t offset)
{
    uint8_t value = base[offset];
    return ++reads == fault_at ? value ^ 1 : value;
}
#define LOADER_COPY_READ(base, offset) test_read(base, offset)
#include "../platforms/sunfish/loader-copy-check.h"

int main(void)
{
    uint8_t source[80], destination[80], snapshot[80];
    for (unsigned n = 0; n < 80; n++) source[n] = (uint8_t)(n * 17);
    memcpy(destination, source, sizeof(source));
    memcpy(snapshot, destination, sizeof(snapshot));
    const volatile uint8_t *s = source + 8, *d = destination + 8;
    assert(loader_copy_verify(s, d, 16, 32, 64) == LOADER_COPY_OK);
    assert(reads == 192 && memcmp(destination, snapshot, sizeof(snapshot)) == 0);
    /* A second-pass discrepancy must fail without repairing either buffer. */
    reads = 0; fault_at = 97;
    assert(loader_copy_verify(s, d, 16, 32, 64) == 0);
    assert(memcmp(destination, snapshot, sizeof(snapshot)) == 0);
    fault_at = 0;
    destination[8 + 20] ^= 1;
    assert(loader_copy_verify(s, d, 16, 32, 64) == LOADER_COPY_OK);
    destination[8 + 5] ^= 1;
    assert(loader_copy_verify(s, d, 16, 32, 64) == 5);
    destination[8 + 5] ^= 1;
    destination[8 + 50] ^= 1;
    assert(loader_copy_verify(s, d, 16, 32, 64) == 50);
    reads = 0;
    assert(loader_copy_verify(s, d, 0, 32, 64) == LOADER_COPY_BOUNDS);
    assert(loader_copy_verify(s, d, 33, 32, 64) == LOADER_COPY_BOUNDS);
    assert(loader_copy_verify(s, d, 16, 65, 64) == LOADER_COPY_BOUNDS);
    assert(loader_copy_verify(s, d, 16, 32, 0x5000001) == LOADER_COPY_BOUNDS);
    assert(loader_copy_verify(s, s, 16, 32, 64) == LOADER_COPY_BOUNDS);
    assert(loader_copy_verify(s, s + 1, 16, 32, 64) == LOADER_COPY_BOUNDS);
    assert(loader_copy_verify(0, d, 16, 32, 64) == LOADER_COPY_BOUNDS);
    assert(loader_copy_verify((const volatile uint8_t *)(UINTPTR_MAX - 2), d,
                              16, 32, 64) == LOADER_COPY_BOUNDS);
    assert(reads == 0);
    assert(memcmp(source, snapshot, sizeof(source)) == 0);
}
