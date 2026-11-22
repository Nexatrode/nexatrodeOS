#pragma once

#include <stdbool.h>
#include <stdint.h>
#include <kernel/gen_config.h>

/* The UI owns the request page. Each poller alone owns its last_delivered. */
static inline bool slint_frame_wake_due(uint64_t request, uint64_t now_ticks,
                                        uint64_t *last_delivered)
{
    if (request == 0 || request == *last_delivered || request > now_ticks) {
        return false;
    }
    *last_delivered = request;
    return true;
}

static inline bool slint_frame_wake_poll(const uint64_t *request, uint64_t *last_delivered)
{
    if (!request) return false;
    uint64_t deadline = __atomic_load_n(request, __ATOMIC_ACQUIRE);
    if (deadline == 0 || deadline == *last_delivered) return false;
    uint64_t now_ticks;
#if defined(CONFIG_EXPORT_VCNT_USER)
    __asm__ volatile("isb\n\tmrs %0, cntvct_el0" : "=r"(now_ticks));
#elif defined(CONFIG_EXPORT_PCNT_USER)
    __asm__ volatile("isb\n\tmrs %0, cntpct_el0" : "=r"(now_ticks));
#else
    return false;
#endif
    return slint_frame_wake_due(deadline, now_ticks, last_delivered);
}
