/* Bounded ordinary-RAM copy/readback. Never use this for device registers. */
#ifndef SUNFISH_LOADER_COPY_PROGRESS_H
#define SUNFISH_LOADER_COPY_PROGRESS_H
#include <stddef.h>
#include <stdint.h>

typedef uint64_t loader_ram_word __attribute__((__may_alias__));
typedef uint32_t loader_ram_half __attribute__((__may_alias__));
enum loader_ram_stage {
    LOADER_RAM_READ, LOADER_RAM_WRITE, LOADER_RAM_COPY,
    LOADER_RAM_VERIFY1, LOADER_RAM_VERIFY2
};
#define LOADER_RAM_OK UINT32_MAX
#define LOADER_RAM_BOUNDS (UINT32_MAX - 1)
#ifndef LOADER_RAM_CHUNK
#define LOADER_RAM_CHUNK ((size_t)0x400000)
#endif
#ifndef LOADER_RAM_READ8
#define LOADER_RAM_READ8(base, offset) ((base)[offset])
#endif
#ifndef LOADER_RAM_READ64
#define LOADER_RAM_READ64(base, offset) (*(const volatile loader_ram_word *)((base) + (offset)))
#endif
#ifndef LOADER_RAM_READ32
#define LOADER_RAM_READ32(base, offset) (*(const volatile loader_ram_half *)((base) + (offset)))
#endif
#ifndef LOADER_RAM_WRITE8
#define LOADER_RAM_WRITE8(base, offset, value) ((base)[offset] = (value))
#endif
#ifndef LOADER_RAM_WRITE64
#define LOADER_RAM_WRITE64(base, offset, value) (*(volatile loader_ram_word *)((base) + (offset)) = (value))
#endif
#ifndef LOADER_RAM_WRITE32
#define LOADER_RAM_WRITE32(base, offset, value) (*(volatile loader_ram_half *)((base) + (offset)) = (value))
#endif

#ifdef SUNFISH_LOADER_COPY_PROGRESS
static void loader_ram_mark(enum loader_ram_stage stage, size_t offset)
{
    char text[25] = "IMAGE COPY ";
    if (stage == LOADER_RAM_READ) { loader_checkpoint("IMAGE COPY READ"); return; }
    if (stage == LOADER_RAM_WRITE) { loader_checkpoint("IMAGE COPY WRITE"); return; }
    unsigned n = 11;
    if (stage == LOADER_RAM_VERIFY1 || stage == LOADER_RAM_VERIFY2) {
        /* Construct on the stack: no absolute-address string-pointer table
         * may be used while this code runs before relocation. */
        text[6] = 'V'; text[7] = 'E'; text[8] = 'R'; text[9] = 'I';
        text[10] = 'F'; text[11] = 'Y';
        text[12] = stage == LOADER_RAM_VERIFY1 ? '1' : '2'; text[13] = ' ';
        n = 14;
    }
    for (unsigned digit = 0; digit < 8; digit++)
        text[n++] = "0123456789ABCDEF"[(offset >> (28 - digit * 4)) & 15];
    text[n] = 0;
    loader_checkpoint(text);
}
#ifndef LOADER_RAM_PROGRESS
#define LOADER_RAM_PROGRESS(stage, offset) loader_ram_mark(stage, offset)
#endif
#endif
#ifndef LOADER_RAM_PROGRESS
#define LOADER_RAM_PROGRESS(stage, offset) ((void)(stage), (void)(offset))
#endif

static inline int loader_ram_bounds(const volatile uint8_t *source,
                                    const volatile uint8_t *destination,
                                    size_t prefix_end, size_t payload_start, size_t total)
{
    uintptr_t a = (uintptr_t)source, b = (uintptr_t)destination;
    return a && b && prefix_end && prefix_end <= payload_start && payload_start <= total &&
        total <= 0x5000000 && a <= UINTPTR_MAX - total && b <= UINTPTR_MAX - total &&
        (a + total <= b || b + total <= a);
}

static inline uint32_t loader_ram_copy(const volatile uint8_t *source,
                                      volatile uint8_t *destination,
                                      size_t prefix_end, size_t payload_start, size_t total)
{
    if (!loader_ram_bounds(source, destination, prefix_end, payload_start, total))
        return LOADER_RAM_BOUNDS;
    size_t offset = 0, next = LOADER_RAM_CHUNK;
    LOADER_RAM_PROGRESS(LOADER_RAM_READ, 0);
    while (offset < total) {
        /* Every word access is aligned at both ends and wholly in range.
         * Volatile accesses prevent vectorization or library substitution. */
        if (!(((uintptr_t)source + offset) & 7) &&
            !(((uintptr_t)destination + offset) & 7) && total - offset >= 8) {
            uint64_t value = LOADER_RAM_READ64(source, offset);
            if (!offset) LOADER_RAM_PROGRESS(LOADER_RAM_WRITE, 0);
            LOADER_RAM_WRITE64(destination, offset, value);
            offset += 8;
        } else if (!(((uintptr_t)source + offset) & 3) &&
                   !(((uintptr_t)destination + offset) & 3) && total - offset >= 4) {
            uint32_t value = LOADER_RAM_READ32(source, offset);
            if (!offset) LOADER_RAM_PROGRESS(LOADER_RAM_WRITE, 0);
            LOADER_RAM_WRITE32(destination, offset, value);
            offset += 4;
        } else {
            uint8_t value = LOADER_RAM_READ8(source, offset);
            if (!offset) LOADER_RAM_PROGRESS(LOADER_RAM_WRITE, 0);
            LOADER_RAM_WRITE8(destination, offset, value);
            offset++;
        }
        if (offset <= 8 || offset >= next || offset == total) {
            LOADER_RAM_PROGRESS(LOADER_RAM_COPY, offset);
            if (offset >= next) next += LOADER_RAM_CHUNK;
        }
    }
    return LOADER_RAM_OK;
}

static inline uint32_t loader_ram_verify(const volatile uint8_t *source,
                                        const volatile uint8_t *destination,
                                        size_t prefix_end, size_t payload_start, size_t total)
{
    if (!loader_ram_bounds(source, destination, prefix_end, payload_start, total))
        return LOADER_RAM_BOUNDS;
    for (unsigned pass = 0; pass < 2; pass++) {
        enum loader_ram_stage stage = pass ? LOADER_RAM_VERIFY2 : LOADER_RAM_VERIFY1;
        LOADER_RAM_PROGRESS(stage, 0);
        size_t next = LOADER_RAM_CHUNK;
        for (unsigned range = 0; range < 2; range++) {
            size_t offset = range ? payload_start : 0;
            size_t end = range ? total : prefix_end;
            while (offset < end) {
                if (!(((uintptr_t)source + offset) & 7) &&
                    !(((uintptr_t)destination + offset) & 7) && end - offset >= 8) {
                    uint64_t a = LOADER_RAM_READ64(source, offset);
                    uint64_t b = LOADER_RAM_READ64(destination, offset);
                    if (a != b) {
                        /* Report from the captured words, without rereading
                         * a transient mismatch. This target is little endian. */
                        for (unsigned byte = 0; byte < 8; byte++)
                            if (((a >> (byte * 8)) & 255) != ((b >> (byte * 8)) & 255))
                                return (uint32_t)(offset + byte);
                    }
                    offset += 8;
                } else if (!(((uintptr_t)source + offset) & 3) &&
                           !(((uintptr_t)destination + offset) & 3) && end - offset >= 4) {
                    uint32_t a = LOADER_RAM_READ32(source, offset);
                    uint32_t b = LOADER_RAM_READ32(destination, offset);
                    if (a != b) {
                        for (unsigned byte = 0; byte < 4; byte++)
                            if (((a >> (byte * 8)) & 255) != ((b >> (byte * 8)) & 255))
                                return (uint32_t)(offset + byte);
                    }
                    offset += 4;
                } else {
                    uint8_t a = LOADER_RAM_READ8(source, offset);
                    uint8_t b = LOADER_RAM_READ8(destination, offset);
                    if (a != b) return (uint32_t)offset;
                    offset++;
                }
                if (offset >= next || offset == end) {
                    LOADER_RAM_PROGRESS(stage, offset);
                    while (next <= offset) next += LOADER_RAM_CHUNK;
                }
            }
        }
    }
    return LOADER_RAM_OK;
}

#ifdef SUNFISH_LOADER_COPY_PROGRESS
_Noreturn void loader_copy_hold_stop(void);
#ifdef SUNFISH_LOADER_COPY_CONTINUE
uintptr_t loader_copy_progress_resume(uintptr_t source, uintptr_t destination,
                                     size_t prefix_end, size_t payload_start, size_t total);
uintptr_t loader_copy_progress_resume(uintptr_t source, uintptr_t destination,
                                     size_t prefix_end, size_t payload_start, size_t total)
#else
_Noreturn void loader_copy_progress_hold(uintptr_t source, uintptr_t destination,
                                        size_t prefix_end, size_t payload_start, size_t total);
_Noreturn void loader_copy_progress_hold(uintptr_t source, uintptr_t destination,
                                        size_t prefix_end, size_t payload_start, size_t total)
#endif
{
    uint32_t result = LOADER_RAM_BOUNDS;
    if (source == 0xb0081000 && destination == 0xc0000000)
        result = loader_ram_copy((const volatile uint8_t *)source,
                                 (volatile uint8_t *)destination, prefix_end, payload_start, total);
    if (result == LOADER_RAM_OK)
        result = loader_ram_verify((const volatile uint8_t *)source,
                                   (const volatile uint8_t *)destination, prefix_end, payload_start, total);
    if (result == LOADER_RAM_OK) {
        loader_checkpoint("IMAGE COPY PASS");
#ifdef SUNFISH_LOADER_COPY_CONTINUE
        return destination;
#endif
    }
    else {
        char text[25] = "IMAGE COPY FAIL ";
        for (unsigned digit = 0; digit < 8; digit++)
            text[16 + digit] = "0123456789ABCDEF"[(result >> (28 - digit * 4)) & 15];
        text[24] = 0;
        loader_checkpoint(text);
    }
    loader_copy_hold_stop();
}
#endif
#endif
