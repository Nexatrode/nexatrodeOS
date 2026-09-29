#include "protocol.h"

/* This page is mapped only in this PD, as specified in system.system. */
struct server_state {
    seL4_Word guard;
    seL4_Word completed_additions;
};

static volatile struct server_state *const state =
    (volatile struct server_state *)C_REFERENCE_PRIVATE_ADDRESS;

void init(void)
{
    state->guard = SERVER_GUARD_VALUE;
    state->completed_additions = 0;
    microkit_dbg_puts("C_REFERENCE: ready; state page mapped only in C reference\n");
}

void notified(microkit_channel channel)
{
    (void)channel;
    microkit_dbg_puts("FAIL: unexpected server notification\n");
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    seL4_Word operation = microkit_msginfo_get_label(request);
    seL4_Word count = microkit_msginfo_get_count(request);

    if (channel != SERVICE_CHANNEL) {
        return microkit_msginfo_new(REPLY_BAD_REQUEST, 0);
    }
    if (operation == REQUEST_COUNT && count == 0) {
        microkit_mr_set(0, state->completed_additions);
        return microkit_msginfo_new(REPLY_OK, 1);
    }
    if (operation == REQUEST_GUARD && count == 0) {
        microkit_mr_set(0, state->guard);
        return microkit_msginfo_new(REPLY_OK, 1);
    }
    if (operation != REQUEST_ADD || count != 2) {
        return microkit_msginfo_new(REPLY_BAD_REQUEST, 0);
    }

    seL4_Word left = microkit_mr_get(0);
    seL4_Word right = microkit_mr_get(1);
    if (left > (~(seL4_Word)0) - right) {
        return microkit_msginfo_new(REPLY_OVERFLOW, 0);
    }
    state->completed_additions++;
    microkit_mr_set(0, left + right);
    return microkit_msginfo_new(REPLY_OK, 1);
}
