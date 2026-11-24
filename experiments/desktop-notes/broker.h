/* The manager owns a Notes namespace cap; the child only has this instance RPC.
 * No caller-supplied namespace, path, pointer or vault key crosses the ABI. */
#include "protocol.h"
static uint8_t private_notes_scratch[PRIVATE_NOTE_RECORD_MAX];
static volatile uint8_t *private_notes_vault = (void *)PRIVATE_NOTE_VAULT_PAGE;
static void private_notes_wipe(void *pointer, size_t size)
{ volatile uint8_t *p = pointer; for (size_t i = 0; i < size; ++i) p[i] = 0; }
static void private_notes_clear(void)
{
    private_notes_wipe(private_notes_scratch, sizeof(private_notes_scratch));
    private_notes_wipe((void *)private_notes_vault, 4096);
    private_notes_page = 0;
}
static microkit_msginfo private_notes_result(unsigned label, uint64_t revision)
{
    private_notes_wipe(private_notes_scratch, sizeof(private_notes_scratch));
    private_notes_wipe((void *)private_notes_vault, 4096);
    microkit_mr_set(0, revision);
    return microkit_msginfo_new(label, label == PRIVATE_NOTE_OK ? 1 : 0);
}
static microkit_msginfo private_notes_request(microkit_channel caller, microkit_msginfo request)
{
    unsigned label = microkit_msginfo_get_label(request), count = microkit_msginfo_get_count(request);
    uint64_t supplied = count ? microkit_mr_get(0) : 0;
    if (!private_notes_page || !private_note_authorized(dynamic_active, device_locked, caller, generation,
            supplied, label, count, seL4_MessageInfo_get_extraCaps(request)))
        return microkit_msginfo_new(PRIVATE_NOTE_DENIED, 0);
    volatile struct private_note_page *page = (void *)private_notes_page;
    if (label == PRIVATE_NOTE_LOAD) {
        private_notes_wipe((void *)page, 4096);
        microkit_msginfo reply = microkit_ppcall(PRIVATE_NOTE_VAULT_CHANNEL, microkit_msginfo_new(0x301, 0));
        if (microkit_msginfo_get_label(reply) != 0x300 || microkit_msginfo_get_count(reply) != 2)
            return private_notes_result(PRIVATE_NOTE_IO, 0);
        uint64_t revision = microkit_mr_get(0);
        if (revision) {
            reply = microkit_ppcall(PRIVATE_NOTE_VAULT_CHANNEL, microkit_msginfo_new(0x30b, 0));
            uint32_t bytes = *(volatile uint32_t *)private_notes_vault;
            if (microkit_msginfo_get_label(reply) != 0x300 || microkit_msginfo_get_count(reply) != 2 ||
                microkit_mr_get(0) != revision || microkit_mr_get(1) != bytes || bytes > sizeof(private_notes_scratch))
                return private_notes_result(PRIVATE_NOTE_IO, 0);
            for (unsigned i = 0; i < bytes; ++i) private_notes_scratch[i] = private_notes_vault[i + 4];
            uint32_t length;
            if (!private_note_decode(private_notes_scratch, bytes, &length)) return private_notes_result(PRIVATE_NOTE_INVALID, 0);
            for (unsigned i = 0; i < length; ++i) page->text[i] = private_notes_scratch[PRIVATE_NOTE_HEADER + i];
            page->length = length;
        }
        line("PRIVATE_NOTES|LOADED|revision=", revision);
        return private_notes_result(PRIVATE_NOTE_OK, revision);
    }
    uint64_t expected = microkit_mr_get(1);
    uint32_t length = page->length;
    if (page->reserved || length > PRIVATE_NOTE_MAX) {
        private_notes_wipe((void *)page, 4096);
        return private_notes_result(PRIVATE_NOTE_INVALID, 0);
    }
    private_note_header(private_notes_scratch, length);
    for (unsigned i = 0; i < length; ++i) private_notes_scratch[PRIVATE_NOTE_HEADER + i] = page->text[i];
    private_notes_wipe((void *)page, 4096);
    private_notes_wipe((void *)private_notes_vault, 4096);
    unsigned bytes = PRIVATE_NOTE_HEADER + length;
    for (unsigned i = 0; i < bytes; ++i) private_notes_vault[i + 4] = private_notes_scratch[i];
    *(volatile uint32_t *)private_notes_vault = bytes;
    microkit_mr_set(0, expected); microkit_mr_set(1, bytes);
    microkit_msginfo reply = microkit_ppcall(PRIVATE_NOTE_VAULT_CHANNEL, microkit_msginfo_new(0x30c, 2));
    unsigned result = microkit_msginfo_get_label(reply);
    if (result == 0x305) return private_notes_result(PRIVATE_NOTE_CONFLICT, 0);
    if (result != 0x300 || microkit_msginfo_get_count(reply) != 1) return private_notes_result(PRIVATE_NOTE_IO, 0);
    uint64_t revision = microkit_mr_get(0);
    line("PRIVATE_NOTES|SAVED|revision=", revision);
    return private_notes_result(PRIVATE_NOTE_OK, revision);
}
static bool private_notes_control(struct input_event_record event)
{
    if (event.device || event.type != 1 || event.reserved || event.value > 2) return false;
    unsigned point = event.code == 68 ? 1 : event.code == 87 ? 2 : event.code == 88 ? 3 : event.code == 62 ? 4 : 0;
    if (!point || device_locked || !dynamic_active) return false;
    if (event.value == 1) {
        microkit_mr_set(0, point);
        microkit_msginfo reply = microkit_ppcall(LOCK_VAULT_CHANNEL, microkit_msginfo_new(0x457, 1));
        if (microkit_msginfo_get_label(reply) != 0x300) fail("arm Notes interruption", 0);
        line("PRIVATE_NOTES|INTERRUPTION_ARMED|point=", point);
    }
    return true;
}
