/* CPU readback of a loader RAM copy. This does not test execution or DMA. */
#ifndef SUNFISH_LOADER_COPY_CHECK_H
#define SUNFISH_LOADER_COPY_CHECK_H
#include <stddef.h>
#include <stdint.h>

#define LOADER_COPY_OK UINT32_MAX
#define LOADER_COPY_BOUNDS (UINT32_MAX - 1)
#ifndef LOADER_COPY_READ
#define LOADER_COPY_READ(base, offset) ((base)[offset])
#endif

static inline uint32_t loader_copy_verify(const volatile uint8_t *source,
                                         const volatile uint8_t *destination,
                                         size_t prefix_end, size_t payload_start,
                                         size_t total)
{
    uintptr_t a = (uintptr_t)source, b = (uintptr_t)destination;
    if (!a || !b || !prefix_end || prefix_end > payload_start || payload_start > total ||
        total > 0x5000000 || a > UINTPTR_MAX - total || b > UINTPTR_MAX - total ||
        !(a + total <= b || b + total <= a))
        return LOADER_COPY_BOUNDS;
    /* The live source/destination stacks occupy BSS, so compare only the
     * immutable code/data prefix and the complete appended loader payload. */
    for (unsigned pass = 0; pass < 2; pass++) {
        for (size_t offset = 0; offset < prefix_end; offset++)
            if (LOADER_COPY_READ(source, offset) != LOADER_COPY_READ(destination, offset))
                return (uint32_t)offset;
        for (size_t offset = payload_start; offset < total; offset++)
            if (LOADER_COPY_READ(source, offset) != LOADER_COPY_READ(destination, offset))
                return (uint32_t)offset;
    }
    return LOADER_COPY_OK;
}

#ifdef SUNFISH_LOADER_COPY_HOLD
_Noreturn void loader_copy_hold_stop(void);
_Noreturn void loader_copy_hold(uintptr_t source, uintptr_t destination,
                               size_t prefix_end, size_t payload_start, size_t total);
_Noreturn void loader_copy_hold(uintptr_t source, uintptr_t destination,
                               size_t prefix_end, size_t payload_start, size_t total)
{
    loader_checkpoint("IMAGE COPY CHECK");
    uint32_t result = LOADER_COPY_BOUNDS;
    if (source == 0xb0081000 && destination == 0xc0000000)
        result = loader_copy_verify((const volatile uint8_t *)source,
                                    (const volatile uint8_t *)destination,
                                    prefix_end, payload_start, total);
    if (result == LOADER_COPY_OK) {
        loader_checkpoint("IMAGE COPY PASS");
    } else {
        char text[25] = "IMAGE COPY FAIL ";
        for (unsigned n = 0; n < 8; n++)
            text[16 + n] = "0123456789ABCDEF"[(result >> (28 - n * 4)) & 15];
        text[24] = 0;
        loader_checkpoint(text);
    }
    loader_copy_hold_stop();
}
#endif
#endif
