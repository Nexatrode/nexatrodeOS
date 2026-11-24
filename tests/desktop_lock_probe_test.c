/* Reproduce a lock/unlock transition between the probe's separate RPCs. */
#include <assert.h>
#include <stdio.h>
#include <string.h>
#define DYNAMIC_WRAPPED_LOCK
#include "../experiments/desktop-lock/probe.c"
static _Alignas(8) uint8_t transfer[4096];
static uint64_t mr[4];
static unsigned scenario, calls, writes, failures, recovered, denied;
microkit_msginfo microkit_msginfo_new(uint64_t label, unsigned count) { return (microkit_msginfo){label, count}; }
uint64_t microkit_msginfo_get_label(microkit_msginfo info) { return info.label; }
unsigned microkit_msginfo_get_count(microkit_msginfo info) { return info.count; }
void microkit_mr_set(unsigned index, uint64_t value) { assert(index < 4); mr[index] = value; }
uint64_t microkit_mr_get(unsigned index) { assert(index < 4); return mr[index]; }
void microkit_dbg_puts(const char *text) {
    if (strstr(text, "LOCK|FAIL|")) ++failures;
    if (strstr(text, "|RECOVERED")) ++recovered;
    if (strstr(text, "|DENIED_AND_CLEAR")) ++denied;
}
microkit_msginfo microkit_ppcall(unsigned channel, microkit_msginfo info) {
    assert(channel == 2); ++calls;
    if (info.label == 0x301) {
        if (scenario == 0 || ((scenario == 1 || scenario == 4) && calls == 1)) return microkit_msginfo_new(0x304, 0);
        mr[0] = scenario == 1 ? 0 : 1;
        return microkit_msginfo_new(0x300, 2);
    }
    if (info.label == 0x30c) {
        assert(info.count == 2);
        if (mr[1] == 0) {
            assert(mr[0] == UINT64_MAX); /* Never allow the denial probe to alter a note. */
            if (scenario == 0) return microkit_msginfo_new(0x304, 0);
            return microkit_msginfo_new(scenario == 4 ? 0x300 : 0x305, 0);
        }
        assert(scenario == 1 && mr[0] == 0 && mr[1] == sizeof(note) - 1);
        assert(!memcmp(transfer + 4, note, sizeof(note) - 1));
        ++writes; return microkit_msginfo_new(0x300, 1);
    }
    if (info.label == 0x30b) {
        if (scenario == 3) { memset(transfer, 0, sizeof(transfer)); return microkit_msginfo_new(0x304, 0); }
        *(uint32_t *)transfer = sizeof(note) - 1;
        memcpy(transfer + 4, note, sizeof(note) - 1);
        return microkit_msginfo_new(0x300, 2);
    }
    assert(info.label == 0x455); return microkit_msginfo_new(0x304, 0);
}
int main(void) {
    notes = transfer;
    for (scenario = 0; scenario < 5; ++scenario) {
        memset(transfer, 0, sizeof(transfer)); memset(mr, 0, sizeof(mr));
        calls = writes = failures = recovered = denied = 0;
        note_probe();
        if (scenario == 4) { assert(failures == 1 && !writes); continue; }
        assert(!failures);
        assert(writes == (scenario == 1));
        assert(recovered == (scenario == 1 || scenario == 2));
        assert(denied == (scenario == 0));
        for (unsigned i = 0; i < sizeof(transfer); ++i) assert(!transfer[i]);
    }
    puts("PASS stable lock, unlock-between-RPCs, relock-between-reads and erroneous-write detection");
}
