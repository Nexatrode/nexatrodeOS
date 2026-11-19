/* Fixed read-only INFO queries for the pinned Sunfish SCM caller.
 * No queried PAS operation is invoked, no buffer address or PAS ID is passed.
 * This is an isolated probe plan, not a generic SMC forwarding service.
 */
#ifndef SUNFISH_SCM_PAS_QUERY_H
#define SUNFISH_SCM_PAS_QUERY_H
#include <stdbool.h>
#include <stdint.h>

#define SUNFISH_SCM_QUERY_COUNT 5u

struct sunfish_scm_registers { uint64_t x[8]; };
enum sunfish_scm_query_error {
    SUNFISH_SCM_QUERY_OK = 0,
    SUNFISH_SCM_QUERY_CONVENTION = 1,
    SUNFISH_SCM_QUERY_TRANSPORT = 2,
    SUNFISH_SCM_QUERY_RESULT = 3,
};
struct sunfish_scm_query_report {
    struct sunfish_scm_registers replies[SUNFISH_SCM_QUERY_COUNT];
    uint32_t issued_mask;
    uint32_t completed_mask;
    uint32_t available_mask;
    uint32_t last_index;
    enum sunfish_scm_query_error error;
};

static inline bool sunfish_scm_query_request(uint32_t index,
                                            struct sunfish_scm_registers *out)
{
    /* The first query exactly matches scm.c is_scm_armv8's SMC64 trial.
     * Later queries match scm_is_call_available -> scm_call2's normal
     * SMC64 INFO envelope. x2 is a scalar *identifier*, never a call.
     */
    static const uint64_t targets[SUNFISH_SCM_QUERY_COUNT] = {
        0x82000601u, 0x02000201u, 0x02000202u, 0x02000205u, 0x02000206u
    };
    if (index >= SUNFISH_SCM_QUERY_COUNT || out == 0) return false;
    *out = (struct sunfish_scm_registers){ .x = {
        index == 0 ? 0xc2000601u : 0x42000601u,
        1u, targets[index], 0, 0, 0, 0, 0
    }};
    return true;
}

typedef void (*sunfish_scm_query_call)(void *, const struct sunfish_scm_registers *,
                                     struct sunfish_scm_registers *);
typedef void (*sunfish_scm_query_progress)(void *, const struct sunfish_scm_query_report *);

/* At most five calls. Stop on the first transport/noncanonical result, with
 * raw registers retained. Do not retry BUSY, resume an interrupted operation,
 * guess an SMC32/legacy fallback, or claim a wall-clock bound on secure world.
 */
static inline struct sunfish_scm_query_report sunfish_scm_query_run(
    void *context, sunfish_scm_query_call call, sunfish_scm_query_progress progress)
{
    struct sunfish_scm_query_report r = {0};
    for (uint32_t i = 0; i < SUNFISH_SCM_QUERY_COUNT; ++i) {
        struct sunfish_scm_registers args;
        (void)sunfish_scm_query_request(i, &args);
        r.last_index = i;
        progress(context, &r); /* Paint before a potentially nonreturning SMC. */
        r.issued_mask |= 1u << i;
        call(context, &args, &r.replies[i]);
        r.completed_mask |= 1u << i;
        if (r.replies[i].x[0] != 0) {
            r.error = SUNFISH_SCM_QUERY_TRANSPORT;
            return r;
        }
        if (r.replies[i].x[1] > 1) {
            r.error = SUNFISH_SCM_QUERY_RESULT;
            return r;
        }
        if (r.replies[i].x[1] == 1) r.available_mask |= 1u << i;
        if (i == 0 && r.replies[i].x[1] != 1) {
            r.error = SUNFISH_SCM_QUERY_CONVENTION;
            return r;
        }
    }
    return r;
}
#endif
