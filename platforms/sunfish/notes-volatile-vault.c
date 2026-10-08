#include <microkit.h>
#include "notes-protocol.h"

/* Explicitly volatile: a private domain-local record, never flash storage. */
static uint8_t stored[NOTES_CAPACITY];
static uint32_t stored_length;

void init(void)
{
    microkit_dbg_puts("SUNFISH VOLATILE NOTES VAULT READY\r\n");
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    if (channel != 0 || microkit_msginfo_get_label(request) != VAULT_WRITE ||
        microkit_msginfo_get_count(request) != 17)
        return microkit_msginfo_new(0, 0);
    seL4_Word length = microkit_mr_get(0);
    if (length > NOTES_CAPACITY) return microkit_msginfo_new(0, 0);
    uint8_t candidate[NOTES_CAPACITY];
    for (uint32_t word_index = 0; word_index < NOTES_CAPACITY / 8; ++word_index) {
        seL4_Word word = microkit_mr_get(word_index + 1);
        for (uint32_t j = 0; j < 8; ++j)
            candidate[word_index * 8 + j] = (uint8_t)(word >> (j * 8));
    }
    for (uint32_t i = 0; i < length; ++i) {
        if (candidate[i] != ' ' && candidate[i] != '\n' &&
            !(candidate[i] >= 'A' && candidate[i] <= 'Z') &&
            !(candidate[i] >= '0' && candidate[i] <= '9'))
            return microkit_msginfo_new(0, 0);
    }
    for (uint32_t i = 0; i < NOTES_CAPACITY; ++i) stored[i] = candidate[i];
    stored_length = (uint32_t)length;
    uint32_t hash = 2166136261u;
    for (uint32_t i = 0; i < stored_length; ++i)
        hash = (hash ^ stored[i]) * 16777619u;
    microkit_dbg_puts("SUNFISH VOLATILE VAULT WRITE\r\n");
    microkit_mr_set(0, stored_length);
    microkit_mr_set(1, hash);
    return microkit_msginfo_new(VAULT_OK, 2);
}

void notified(microkit_channel channel) { (void)channel; }
