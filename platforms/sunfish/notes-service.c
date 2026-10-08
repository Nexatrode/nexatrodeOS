#include <microkit.h>
#include "notes-protocol.h"

/* Only this protection domain owns the editable state. */
static uint8_t note[NOTES_CAPACITY];
static uint32_t length;
static uint32_t status;
static volatile struct notes_view *const view = (void *)0x40000000;

static void publish(void)
{
    view->revision++;
    __atomic_thread_fence(__ATOMIC_RELEASE);
    view->len = length;
    view->status = status;
    for (uint32_t i = 0; i < NOTES_CAPACITY; ++i) {
        view->text[i] = i < length ? note[i] : 0;
    }
    __atomic_thread_fence(__ATOMIC_RELEASE);
    view->revision++;
}

void init(void)
{
    length = 0;
    status = 0;
    view->revision = 0;
    publish();
    microkit_dbg_puts("SUNFISH NOTES SERVICE READY\r\n");
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    if (channel != 0) return microkit_msginfo_new(0, 0);
    seL4_Word label = microkit_msginfo_get_label(request);
    seL4_Word count = microkit_msginfo_get_count(request);
    if (label == NOTES_KEY && count == 1) {
        seL4_Word key = microkit_mr_get(0);
        if (key == 0x100) {
            if (length) length--;
        } else if (key == 0x101 || key == 0x102 ||
                   (key <= 0x7f && ((key >= 'A' && key <= 'Z') ||
                                    (key >= '0' && key <= '9')))) {
            if (length < NOTES_CAPACITY)
                note[length++] = key == 0x101 ? ' ' : key == 0x102 ? '\n' : (uint8_t)key;
        } else {
            return microkit_msginfo_new(0, 0);
        }
        status = 0;
        publish();
        return microkit_msginfo_new(NOTES_OK, 0);
    }
    if (label == NOTES_SAVE && count == 0) {
        uint32_t expected_hash = 2166136261u;
        for (uint32_t i = 0; i < length; ++i)
            expected_hash = (expected_hash ^ note[i]) * 16777619u;
        microkit_mr_set(0, length);
        for (uint32_t word_index = 0; word_index < NOTES_CAPACITY / 8; ++word_index) {
            seL4_Word word = 0;
            for (uint32_t j = 0; j < 8; ++j) {
                uint32_t index = word_index * 8 + j;
                if (index < length) word |= (seL4_Word)note[index] << (j * 8);
            }
            microkit_mr_set(word_index + 1, word);
        }
        microkit_msginfo reply = microkit_ppcall(1, microkit_msginfo_new(VAULT_WRITE, 17));
        status = microkit_msginfo_get_label(reply) == VAULT_OK &&
                 microkit_msginfo_get_count(reply) == 2 &&
                 microkit_mr_get(0) == length &&
                 microkit_mr_get(1) == expected_hash ? 1 : 2;
        publish();
        microkit_dbg_puts(status == 1 ? "SUNFISH NOTES VAULT ACCEPTED\r\n" :
                                      "SUNFISH NOTES VAULT ERROR\r\n");
        return microkit_msginfo_new(NOTES_OK, 0);
    }
    return microkit_msginfo_new(0, 0);
}

void notified(microkit_channel channel) { (void)channel; }
