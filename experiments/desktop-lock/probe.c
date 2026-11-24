#include <microkit.h>
#ifdef DYNAMIC_WRAPPED_LOCK
#include <stdint.h>
/* A static adversarial fixture, not a real app. It holds no vault control cap. */
static volatile uint8_t *notes = (void *)0x50000000;
static const char note[] = "disposable wrapped lifecycle note";
static void clear_note_page(void) { for (unsigned i = 0; i < 4096; ++i) notes[i] = 0; }
static void note_probe(void) {
    microkit_msginfo reply = microkit_ppcall(2, microkit_msginfo_new(0x301, 0));
    unsigned label = microkit_msginfo_get_label(reply);
    if (label == 0x304) { /* REPLY_DENIED */
        for (unsigned i = 0; i < 4096; ++i)
            if (notes[i]) { microkit_dbg_puts("LOCK|FAIL|locked note transfer retained data\n"); return; }
        microkit_mr_set(0, 0); microkit_mr_set(1, 0);
        reply = microkit_ppcall(2, microkit_msginfo_new(0x30c, 2));
        if (microkit_msginfo_get_label(reply) != 0x304) microkit_dbg_puts("LOCK|FAIL|locked note write allowed\n");
        else microkit_dbg_puts("LOCK_VAULT_PROBE|DENIED_AND_CLEAR\n");
        return;
    }
    if (label != 0x300 || microkit_msginfo_get_count(reply) != 2) { microkit_dbg_puts("LOCK|FAIL|note read status\n"); return; }
    uint64_t generation = microkit_mr_get(0);
    if (!generation) {
        clear_note_page();
        *(volatile uint32_t *)notes = sizeof(note) - 1;
        for (unsigned i = 0; i < sizeof(note) - 1; ++i) notes[i + 4] = note[i];
        microkit_mr_set(0, generation); microkit_mr_set(1, sizeof(note) - 1);
        reply = microkit_ppcall(2, microkit_msginfo_new(0x30c, 2));
        if (microkit_msginfo_get_label(reply) != 0x300) { microkit_dbg_puts("LOCK|FAIL|note write\n"); return; }
    }
    reply = microkit_ppcall(2, microkit_msginfo_new(0x30b, 0));
    if (microkit_msginfo_get_label(reply) != 0x300 || microkit_msginfo_get_count(reply) != 2 ||
        *(volatile uint32_t *)notes != sizeof(note) - 1) { microkit_dbg_puts("LOCK|FAIL|note blob read\n"); return; }
    for (unsigned i = 0; i < sizeof(note) - 1; ++i)
        if (notes[i + 4] != (uint8_t)note[i]) { microkit_dbg_puts("LOCK|FAIL|note content mismatch\n"); return; }
    clear_note_page();
    microkit_mr_set(0, 1);
    reply = microkit_ppcall(2, microkit_msginfo_new(0x455, 1));
    if (microkit_msginfo_get_label(reply) != 0x304) { microkit_dbg_puts("LOCK|FAIL|app committed vault unlock\n"); return; }
    microkit_dbg_puts("LOCK_VAULT_PROBE|RECOVERED\n");
}
#endif
static void attempt(void) {
    microkit_mr_set(0, 1);
    microkit_msginfo reply = microkit_ppcall(0, microkit_msginfo_new(0x791, 1));
    if (microkit_msginfo_get_label(reply) != 1) microkit_dbg_puts("LOCK|FAIL|foreign unlock accepted\n");
    else microkit_dbg_puts("LOCK_PROBE|UNLOCK_DENIED\n");
#ifdef DYNAMIC_WRAPPED_LOCK
    note_probe();
#endif
}
void init(void) { attempt(); }
void notified(microkit_channel channel) { if (channel == 1) attempt(); }
