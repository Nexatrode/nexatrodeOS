/* Parent-managed trusted services. Only built by the disposable SimpleX lock
 * fixture. Unlike a suspended app, every writable image/stack/IPC page, worker
 * heap and shared queue is erased before lock completion. Fixed service caps
 * remain in their curated CSpaces; this is not dynamic authority revocation. */
#include "session_layout.h"
#define SESSION_SNAPSHOT_BYTES (16 * 1024 * 1024)
#define SESSION_MAX_BOOT_PAGES 4096
static uint8_t session_boot_bytes[SESSION_SNAPSHOT_BYTES];
static struct { uintptr_t address; } session_boot_pages[SESSION_MAX_BOOT_PAGES];
static unsigned session_boot_used;
static bool session_failed;
static bool session_notifications_bound = true;
struct session_state { uint64_t epoch, active; };
static volatile struct session_state *const session_state = (void *)SESSION_STATE_ADDRESS;
#ifdef DYNAMIC_SIMPLEX_LIVE
#include "../../tests/net/include/simplex_lock_protocol.h"
static bool simplex_net_transition(bool unlock)
{
    microkit_mr_set(0, lock_epoch);
    microkit_msginfo reply = microkit_ppcall(17, microkit_msginfo_new(unlock ? SIMPLEX_NET_UNLOCK : SIMPLEX_NET_LOCK, 1));
    bool ok = microkit_msginfo_get_label(reply) == SIMPLEX_NET_OK && microkit_msginfo_get_count(reply) == 3 &&
        microkit_mr_get(0) == lock_epoch && microkit_mr_get(1) == unlock && microkit_mr_get(2) == 1;
    if (!ok) session_failed = true;
    else line(unlock ? "SIMPLEX_SESSION|NET_OPEN|epoch=" : "SIMPLEX_SESSION|NET_CLOSED|epoch=", lock_epoch);
    return ok;
}
#endif

static bool session_page_nonzero(const volatile uint8_t *p)
{
    for (unsigned i = 0; i < 4096; ++i) if (p[i]) return true;
    return false;
}
static void session_clean(const struct session_range *range, bool invalidate)
{
    /* seL4's ARM VSpace flush may not cross a mapped frame boundary. 4 KiB
     * slices are valid for both the large ELF pages and small tail/stack pages. */
    for (size_t n = 0; n < range->bytes; n += 4096) {
        uintptr_t address = range->address + n;
        check(invalidate ? seL4_ARM_VSpace_CleanInvalidate_Data(ROOT_VSPACE, address, address + 4096)
                         : seL4_ARM_VSpace_Clean_Data(ROOT_VSPACE, address, address + 4096),
              "SimpleX page cache");
    }
}
static void simplex_session_suspend(void)
{
    session_state->active = 0;
    __atomic_thread_fence(__ATOMIC_SEQ_CST);
    for (unsigned i = 0; i < SESSION_CHILD_COUNT; ++i)
        check(seL4_TCB_Suspend(BASE_TCB_CAP + SESSION_FIRST_CHILD + i), "SimpleX quiesce");
    if (session_notifications_bound) {
        for (unsigned i = 0; i < SESSION_CHILD_COUNT; ++i)
            check(seL4_TCB_UnbindNotification(BASE_TCB_CAP + SESSION_FIRST_CHILD + i), "SimpleX unbind wake");
        session_notifications_bound = false;
    }
#ifdef DYNAMIC_SIMPLEX_LIVE
    (void)simplex_net_transition(false);
#endif
}
static void simplex_session_scrub(void)
{
    /* Suspend cancels IPC but does not return a donated scheduling context.
     * The vault has now completed key eviction. Detach every service SC from
     * old call/reply chains before reconstructing the stopped threads. */
    for (unsigned i = 0; i < SESSION_CHILD_COUNT; ++i)
        check(seL4_SchedContext_Unbind((SESSION_SC_CAP_BASE + (seL4_Word)i) << 58), "SimpleX detach budget");
    size_t total = 0;
    for (unsigned i = 0; i < SESSION_RANGE_COUNT; ++i) {
        const struct session_range *range = &session_ranges[i];
        volatile uint8_t *p = (void *)range->address;
        for (size_t n = 0; n < range->bytes; ++n) p[n] = 0;
        session_clean(range, true);
        for (size_t n = 0; n < range->bytes; ++n) if (p[n]) {
            session_failed = true;
            print("SIMPLEX_SESSION|ERASURE_FAILED\n");
            return;
        }
        total += range->bytes;
    }
    /* TCB_Suspend cancels blocked IPC. Discard latched notifications before
     * restart. A later timer wake sees empty queues; no producer of their
     * payloads exists outside this stopped group in the offline fixture. */
    for (unsigned i = 0; i < SESSION_CHILD_COUNT; ++i) {
        seL4_Word badge;
        (void)seL4_Poll((8UL + i) << 58, &badge);
        seL4_UserContext context = {0};
        check(seL4_TCB_WriteRegisters(BASE_TCB_CAP + SESSION_FIRST_CHILD + i, false,
              0, sizeof(context) / sizeof(seL4_Word), &context), "SimpleX clear registers");
        check(seL4_TCB_SetTLSBase(BASE_TCB_CAP + SESSION_FIRST_CHILD + i, 0), "SimpleX clear TLS");
    }
    session_state->epoch = lock_epoch;
    line("SIMPLEX_SESSION|SCRUBBED|bytes=", total);
}
static void simplex_session_boot(void)
{
    /* The composer starts all six children suspended. Capture only nonzero
     * image pages: patched addresses/configuration, never a running heap. */
    for (unsigned i = 0; i < SESSION_RANGE_COUNT; ++i) {
        const struct session_range *range = &session_ranges[i];
        for (size_t n = 0; n < range->bytes; n += 4096) {
            uintptr_t address = range->address + n;
            if (!session_page_nonzero((void *)address)) continue;
            if (session_boot_used == SESSION_MAX_BOOT_PAGES) fail("SimpleX boot snapshot quota", 0);
            session_boot_pages[session_boot_used].address = address;
            copy(session_boot_bytes + session_boot_used * 4096, (void *)address, 4096);
            ++session_boot_used;
        }
    }
    simplex_session_suspend();
    simplex_session_scrub();
    print("SIMPLEX_SESSION|BOOT_LOCKED\n");
}
static bool simplex_session_start(void)
{
    if (session_failed) return false;
    for (unsigned i = 0; i < session_boot_used; ++i)
        copy((void *)session_boot_pages[i].address, session_boot_bytes + i * 4096, 4096);
    session_state->epoch = lock_epoch;
    session_state->active = 1;
    __atomic_thread_fence(__ATOMIC_SEQ_CST);
    for (unsigned i = 0; i < SESSION_RANGE_COUNT; ++i) {
        const struct session_range *range = &session_ranges[i];
        session_clean(range, false);
    }
    for (unsigned i = 0; i < SESSION_CHILD_COUNT; ++i) {
        const struct session_child *child = &session_children[i];
        seL4_UserContext context = { .pc = child->entry, .sp = SESSION_STACK_TOP };
        check(seL4_TCB_WriteRegisters(BASE_TCB_CAP + SESSION_FIRST_CHILD + i, false,
              0, sizeof(context) / sizeof(seL4_Word), &context), "SimpleX restart context");
        check(seL4_TCB_BindNotification(BASE_TCB_CAP + SESSION_FIRST_CHILD + i,
              (8UL + i) << 58), "SimpleX bind wake");
    }
    session_notifications_bound = true;
    for (unsigned i = 0; i < SESSION_CHILD_COUNT; ++i)
        check(seL4_SchedContext_Bind((SESSION_SC_CAP_BASE + (seL4_Word)i) << 58,
              BASE_TCB_CAP + SESSION_FIRST_CHILD + i), "SimpleX restore budget");
#ifdef DYNAMIC_SIMPLEX_LIVE
    if (!simplex_net_transition(true)) return false;
#endif
    /* Reconstruct service state only after the vault commit; fresh initialization
     * reopens AgentStore and rebuilds transcript indexes from encrypted storage. */
    for (unsigned i = 0; i < SESSION_CHILD_COUNT; ++i)
        check(seL4_TCB_Resume(BASE_TCB_CAP + SESSION_FIRST_CHILD + i), "SimpleX restart");
    line("SIMPLEX_SESSION|STARTED|epoch=", lock_epoch);
    return true;
}
