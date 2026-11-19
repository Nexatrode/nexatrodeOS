/* QEMU proof of the same monotonic-time hook used by the GHC RTS adapter. */

#include <stdbool.h>
#include <stdint.h>

#include <microkit.h>
#include <sddf/timer/client.h>
#include <sddf/util/printf.h>

#include "../sddf_time_bridge.c"

static uint64_t start_ns;
static bool woke;
void notified(microkit_channel channel);

void init(void)
{
    if (!sel4_rts_read_monotonic_ns(&start_ns)) {
        sddf_printf("GHCTIMER|FAIL: timer capability missing\n");
        return;
    }

    sddf_printf("GHCTIMER|START ns=%lu\n", (unsigned long)start_ns);
    sel4_rts_wait_for_event(start_ns + NS_IN_S);
    if (!woke) {
        sddf_printf("GHCTIMER|FAIL: wait hook returned without timer callback\n");
    }
}

void notified(microkit_channel channel)
{
    uint64_t now_ns;
    if (channel != ghc_timer_config.driver_id ||
        !sel4_rts_read_monotonic_ns(&now_ns) || now_ns <= start_ns) {
        sddf_printf("GHCTIMER|FAIL: invalid or early wake channel=%u\n", channel);
        return;
    }

    woke = true;
    sddf_printf("GHCTIMER|WAIT_DISPATCH_OK channel=%u\n", channel);
    sddf_printf("GHCTIMER|WAKE_OK elapsed_ns=%lu\n",
                (unsigned long)(now_ns - start_ns));
}
