/* Disposable wrapped-vault experiment: defer GHC/SQLite startup until unlock.
 * A seL4 notification is latched between the check and wait. Every wake is
 * followed by a fresh status check, so spurious and stale notifications do
 * not make the agent start against a locked vault. */
#include <microkit.h>
#include <stdint.h>

enum { VAULT_CHANNEL = 0, VAULT_STATUS = 0x454, VAULT_READY = 0x300,
       UNLOCK_NOTIFICATION = 6 };

void simplex_wait_for_vault(void)
{
    microkit_dbg_puts("SIMPLEX_GATE|WAITING\n");
    for (;;) {
        microkit_msginfo reply = microkit_ppcall(VAULT_CHANNEL,
                                                  microkit_msginfo_new(VAULT_STATUS, 0));
        if (microkit_msginfo_get_label(reply) == VAULT_READY &&
            microkit_msginfo_get_count(reply) == 0) {
            microkit_dbg_puts("SIMPLEX_GATE|STARTING\n");
            return;
        }
        seL4_Word badge;
        do {
            (void)seL4_Wait(1, &badge);
        } while ((badge & (UINT64_C(1) << UNLOCK_NOTIFICATION)) == 0);
    }
}
