/* Bounded Microkit RNG adapter for the native Haskell protection domain. */
#include <stdint.h>
#include <microkit.h>

#ifndef ENTROPY_CHANNEL
#define ENTROPY_CHANNEL 1
#endif
#define ENTROPY_REQUEST_BYTES 0x401
#define ENTROPY_REPLY_OK 0x400

int sel4_haskell_entropy16(uint8_t *out)
{
    if (out == 0) return 0;
    microkit_msginfo reply = microkit_ppcall(
        ENTROPY_CHANNEL, microkit_msginfo_new(ENTROPY_REQUEST_BYTES, 0));
    if (microkit_msginfo_get_label(reply) != ENTROPY_REPLY_OK ||
        microkit_msginfo_get_count(reply) != 2)
        return 0;
    for (unsigned word_index = 0; word_index < 2; ++word_index) {
        seL4_Word word = microkit_mr_get(word_index);
        for (unsigned byte_index = 0; byte_index < 8; ++byte_index)
            out[word_index * 8 + byte_index] = (uint8_t)(word >> (8 * byte_index));
    }
    return 1;
}

/* crypton requests arbitrary lengths; the isolated RNG protocol returns 16
 * bytes per protected call. Report partial progress so crypton can fail closed
 * if the service stops responding. */
int sel4_haskell_entropy_bytes(uint8_t *out, int count)
{
    if (out == 0 || count < 0) return 0;
    int filled = 0;
    while (filled < count) {
        uint8_t block[16];
        if (!sel4_haskell_entropy16(block)) return filled;
        int chunk = count - filled < 16 ? count - filled : 16;
        for (int i = 0; i < chunk; ++i) out[filled + i] = block[i];
        for (int i = 0; i < 16; ++i) ((volatile uint8_t *)block)[i] = 0;
        filled += chunk;
    }
    return filled;
}
