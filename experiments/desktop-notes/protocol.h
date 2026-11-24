/* Bounded private Notes ABI for the disposable dynamic-app experiment. */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#define PRIVATE_NOTE_LOAD 0x7a0u
#define PRIVATE_NOTE_SAVE 0x7a1u
#define PRIVATE_NOTE_MAX 2048u
#define PRIVATE_NOTE_HEADER 16u
#define PRIVATE_NOTE_RECORD_MAX (PRIVATE_NOTE_HEADER + PRIVATE_NOTE_MAX)
#define PRIVATE_NOTE_VAULT_CHANNEL 14u
#define PRIVATE_NOTE_PAGE 0x6a000000UL
#define PRIVATE_NOTE_PARENT_PAGE 0x6b000000UL
#define PRIVATE_NOTE_VAULT_PAGE 0x6c000000UL
#define PRIVATE_NOTE_OK 0u
#define PRIVATE_NOTE_DENIED 1u
#define PRIVATE_NOTE_INVALID 2u
#define PRIVATE_NOTE_IO 3u
#define PRIVATE_NOTE_CONFLICT 4u
struct private_note_page { uint32_t length, reserved; uint8_t text[PRIVATE_NOTE_MAX]; };
_Static_assert(sizeof(struct private_note_page) <= 4096, "one private exchange page");
static inline bool private_note_authorized(bool active, bool locked, uint64_t caller, uint64_t instance,
                                           uint64_t supplied, unsigned label, unsigned count, unsigned extra)
{
    return active && !locked && instance && caller == instance && supplied == instance && !extra &&
           ((label == PRIVATE_NOTE_LOAD && count == 1) || (label == PRIVATE_NOTE_SAVE && count == 2));
}
static inline uint32_t private_note_u32(const uint8_t *p)
{ return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24; }
static inline void private_note_header(uint8_t *p, uint32_t length)
{
    const uint8_t magic[8] = {'N','X','N','O','T','E','0','1'};
    for (unsigned i = 0; i < 8; ++i) p[i] = magic[i];
    for (unsigned i = 0; i < 4; ++i) { p[8+i] = length >> (8*i); p[12+i] = 0; }
}
static inline bool private_note_decode(const uint8_t *p, size_t size, uint32_t *length)
{
    const uint8_t magic[8] = {'N','X','N','O','T','E','0','1'};
    if (size < PRIVATE_NOTE_HEADER || size > PRIVATE_NOTE_RECORD_MAX) return false;
    for (unsigned i = 0; i < 8; ++i) if (p[i] != magic[i]) return false;
    uint32_t n = private_note_u32(p + 8);
    if (n > PRIVATE_NOTE_MAX || private_note_u32(p + 12) || size != PRIVATE_NOTE_HEADER + n) return false;
    *length = n;
    return true;
}
