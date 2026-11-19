#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include "../platforms/sunfish/scm_pas_query.h"

struct fake {
    uint32_t calls, progress;
    int fault_at;
    uint64_t status, value;
};
static void progress(void *context, const struct sunfish_scm_query_report *r)
{
    struct fake *f = context;
    assert(r->last_index == f->calls);
    assert(r->completed_mask == (1u << f->calls) - 1);
    f->progress++;
}
static void call(void *context, const struct sunfish_scm_registers *in,
                 struct sunfish_scm_registers *out)
{
    struct fake *f = context;
    const uint64_t targets[] = {0x82000601, 0x02000201, 0x02000202, 0x02000205, 0x02000206};
    assert(f->calls < 5 && f->progress == f->calls + 1);
    assert(in->x[0] == (f->calls == 0 ? 0xc2000601u : 0x42000601u));
    assert(in->x[1] == 1 && in->x[2] == targets[f->calls]);
    for (unsigned j = 3; j < 8; ++j) assert(in->x[j] == 0);
    /* Actual PAS IDs are never in x0; all requests invoke INFO/IS_CALL_AVAIL. */
    assert((in->x[0] & 0xffffu) == 0x0601u);
    *out = (struct sunfish_scm_registers){ .x = {0, 1, 0x1234, 0x5678, 0, 0, 0, 0} };
    if ((int)f->calls == f->fault_at) {
        out->x[0] = f->status;
        out->x[1] = f->value;
    }
    f->calls++;
}
static struct sunfish_scm_query_report run(struct fake *f) {
    return sunfish_scm_query_run(f, call, progress);
}
static void all_available_and_fixed_envelopes(void)
{
    struct fake f = { .fault_at = -1 };
    struct sunfish_scm_query_report r = run(&f);
    assert(r.error == SUNFISH_SCM_QUERY_OK);
    assert(f.calls == 5 && r.issued_mask == 31 && r.completed_mask == 31);
    assert(r.available_mask == 31);
    for (unsigned i = 0; i < 5; ++i) assert(r.replies[i].x[2] == 0x1234 && r.replies[i].x[3] == 0x5678);
    struct sunfish_scm_registers request = {.x={99}};
    assert(!sunfish_scm_query_request(5, &request) && request.x[0] == 99);
    assert(!sunfish_scm_query_request(UINT32_MAX, &request));
    assert(!sunfish_scm_query_request(0, 0));
}
static void unavailable_is_distinct_from_transport_error(void)
{
    for (int i = 1; i < 5; ++i) {
        struct fake f = { .fault_at=i, .status=0, .value=0 };
        struct sunfish_scm_query_report r = run(&f);
        assert(r.error == SUNFISH_SCM_QUERY_OK && f.calls == 5);
        assert(r.available_mask == (31u & ~(1u << i)));
    }
    struct fake f = { .fault_at=0, .status=0, .value=0 };
    struct sunfish_scm_query_report r = run(&f);
    assert(r.error == SUNFISH_SCM_QUERY_CONVENTION && f.calls == 1);
}
static void every_transport_fault_is_terminal_with_full_raw_values(void)
{
    for (int i = 0; i < 5; ++i) {
        for (unsigned j = 0; j < 4; ++j) {
            const uint64_t statuses[] = {1, UINT64_MAX, (uint64_t)-12, 1ull << 32};
            struct fake f = { .fault_at=i, .status=statuses[j], .value=1 };
            struct sunfish_scm_query_report r = run(&f);
            assert(r.error == SUNFISH_SCM_QUERY_TRANSPORT && f.calls == (uint32_t)i + 1);
            assert(r.replies[i].x[0] == statuses[j]);
            assert(r.completed_mask == (1u << (i+1)) - 1);
            for (int next = i+1; next < 5; ++next) assert(r.replies[next].x[0] == 0 && r.replies[next].x[1] == 0);
        }
    }
}
static void noncanonical_result_never_becomes_available(void)
{
    for (int i = 0; i < 5; ++i) {
        const uint64_t values[] = {2, UINT64_MAX, (1ull << 32) | 1};
        for (unsigned j = 0; j < 3; ++j) {
            struct fake f = { .fault_at=i, .status=0, .value=values[j] };
            struct sunfish_scm_query_report r = run(&f);
            assert(r.error == SUNFISH_SCM_QUERY_RESULT && f.calls == (uint32_t)i + 1);
            assert((r.available_mask & (1u << i)) == 0 && r.replies[i].x[1] == values[j]);
        }
    }
}
int main(void) {
    all_available_and_fixed_envelopes();
    unavailable_is_distinct_from_transport_error();
    every_transport_fault_is_terminal_with_full_raw_values();
    noncanonical_result_never_becomes_available();
    puts("SCM PAS query: 4 host test groups passed");
}
