/*
 * Optional GHC time-hook adapter for PDs configured as sDDF timer clients.
 * This supplies monotonic time only; Unix epoch time remains a separate
 * trusted clock service.
 */

#include <stdbool.h>
#include <stdint.h>

#include <microkit.h>
#include <os/sddf.h>
#include <sddf/timer/client.h>
#include <sddf/timer/config.h>

__attribute__((__section__(".timer_client_config"))) timer_client_config_t
    ghc_timer_config;

static void rts_wait_fail(const char *message)
{
    microkit_dbg_puts("GHC RTS wait: ");
    microkit_dbg_puts(message);
    microkit_dbg_puts("\n");
    __builtin_trap();
}

bool sel4_rts_read_monotonic_ns(uint64_t *nsec)
{
    if (nsec == NULL || !timer_config_check_magic(&ghc_timer_config)) {
        return false;
    }

    *nsec = sddf_timer_time_now(ghc_timer_config.driver_id);
    return true;
}

/*
 * First non-threaded RTS wait bridge. A seL4 notification badge is a bitset,
 * so more than one Microkit channel can be pending when the RTS wakes. Hand
 * every pending channel back to the component callback instead of assuming
 * the timer arrived alone. This is enough for the current synchronous probe;
 * a production event loop still needs deferred IRQ acknowledgement semantics.
 */
void sel4_rts_wait_for_event(uint64_t deadline_ns)
{
    if (!timer_config_check_magic(&ghc_timer_config)) {
        rts_wait_fail("sDDF timer client is not configured");
    }

    unsigned int timer_channel = ghc_timer_config.driver_id;
    if (deadline_ns != UINT64_MAX) {
        uint64_t now_ns = sddf_timer_time_now(timer_channel);
        if (deadline_ns <= now_ns) {
            return;
        }
        sddf_timer_set_timeout(timer_channel, deadline_ns - now_ns);
    }

    seL4_Word badge = 0;
    (void)seL4_Wait(1, &badge);

    /* Dispatch every pending channel synchronously while RTS is idle in C. */
    for (microkit_channel channel = 0; channel < 64; ++channel) {
        if ((badge & (UINT64_C(1) << channel)) != 0) {
            notified(channel);
        }
    }
}
