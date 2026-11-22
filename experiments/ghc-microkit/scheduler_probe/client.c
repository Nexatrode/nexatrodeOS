/* Exercise the patched GHC awaitEvent against real RTS Capability/TSO types. */

#include <stdbool.h>
#include <stdint.h>

#include <microkit.h>
#include <sddf/timer/config.h>

#include "Rts.h"
#include "Schedule.h"
#include "Capability.h"
#include "IOManager.h"
#include "GetTime.h"
#include "posix/Select.h"

/* The harness links the actual GHC awaitEvent object without the full RTS. */
extern timer_client_config_t ghc_timer_config;
StgClosure stg_END_TSO_QUEUE_closure;

static Capability test_cap;
static CapIOManager test_iomgr;
static StgTSO sleeping_tso;
static uint64_t start_ns;
static bool saw_timer_callback;

void awaitEvent(Capability *cap, bool wait);
bool sel4_rts_read_unix_time(StgWord64 *sec, StgWord32 *nsec);

void barf(const char *format, ...)
{
    (void)format;
    microkit_dbg_puts("GHCSCHED|FAIL: RTS barf\n");
    __builtin_trap();
}

void pushOnRunQueue(Capability *cap, StgTSO *tso)
{
    cap->run_queue_hd = tso;
    cap->run_queue_tl = tso;
    cap->n_run_queue++;
}

bool sel4_rts_read_unix_time(StgWord64 *sec, StgWord32 *nsec)
{
    (void)sec;
    (void)nsec;
    return false;
}

void notified(microkit_channel channel)
{
    if (channel != ghc_timer_config.driver_id) {
        microkit_dbg_puts("GHCSCHED|FAIL: unexpected callback channel\n");
        __builtin_trap();
    }
    saw_timer_callback = true;
}

void init(void)
{
    if (!timer_config_check_magic(&ghc_timer_config)) {
        microkit_dbg_puts("GHCSCHED|FAIL: timer capability missing\n");
        return;
    }

    start_ns = getMonotonicNSec();
    test_iomgr.blocked_queue_hd = END_TSO_QUEUE;
    test_iomgr.blocked_queue_tl = END_TSO_QUEUE;
    test_iomgr.sleeping_queue = &sleeping_tso;

    sleeping_tso._link = END_TSO_QUEUE;
    sleeping_tso.why_blocked = BlockedOnDelay;
    sleeping_tso.block_info.target = (LowResTime)(start_ns + 400000000);

    test_cap.iomgr = &test_iomgr;
    test_cap.run_queue_hd = END_TSO_QUEUE;
    test_cap.run_queue_tl = END_TSO_QUEUE;
    test_cap.n_run_queue = 0;

    microkit_dbg_puts("GHCSCHED|START waiting for sleeping TSO deadline\n");
    awaitEvent(&test_cap, true);

    uint64_t end_ns = getMonotonicNSec();
    if (!saw_timer_callback || test_iomgr.sleeping_queue != END_TSO_QUEUE ||
        test_cap.n_run_queue != 1 || test_cap.run_queue_hd != &sleeping_tso ||
        sleeping_tso.why_blocked != NotBlocked ||
        sleeping_tso._link != END_TSO_QUEUE || end_ns < sleeping_tso.block_info.target) {
        microkit_dbg_puts("GHCSCHED|FAIL: RTS awaitEvent did not retire the timer TSO\n");
        return;
    }

    microkit_dbg_puts("GHCSCHED|AWAIT_EVENT_OK queue=1 state=RUNNABLE\n");
}
