#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <microkit.h>

void simplex_wait_for_vault(void);
static microkit_msginfo responses[8];
static seL4_Word badges[8];
static size_t response_index, badge_index, response_count, badge_count;
static unsigned waiting_logs, starting_logs;

microkit_msginfo microkit_msginfo_new(seL4_Word label, unsigned count) { return (microkit_msginfo){label, count}; }
seL4_Word microkit_msginfo_get_label(microkit_msginfo value) { return value.label; }
unsigned microkit_msginfo_get_count(microkit_msginfo value) { return value.count; }
microkit_msginfo microkit_ppcall(unsigned channel, microkit_msginfo request) {
    assert(channel == 0 && request.label == 0x454 && request.count == 0);
    assert(response_index < response_count);
    return responses[response_index++];
}
int seL4_Wait(unsigned cap, seL4_Word *badge) {
    assert(cap == 1 && badge_index < badge_count);
    *badge = badges[badge_index++];
    return 0;
}
void microkit_dbg_puts(const char *message) {
    if (!strcmp(message, "SIMPLEX_GATE|WAITING\n")) waiting_logs++;
    else if (!strcmp(message, "SIMPLEX_GATE|STARTING\n")) starting_logs++;
    else assert(0);
}
static void reset(void) { response_index = badge_index = response_count = badge_count = waiting_logs = starting_logs = 0; }

int main(void) {
    reset();
    responses[response_count++] = (microkit_msginfo){0x300, 0};
    simplex_wait_for_vault();
    assert(response_index == 1 && badge_index == 0 && waiting_logs == 1 && starting_logs == 1);

    reset();
    responses[response_count++] = (microkit_msginfo){0x304, 0};
    responses[response_count++] = (microkit_msginfo){0x300, 1}; /* malformed success */
    responses[response_count++] = (microkit_msginfo){0x304, 0}; /* stale wake */
    responses[response_count++] = (microkit_msginfo){0x300, 0};
    badges[badge_count++] = (seL4_Word)1 << 5; /* unrelated */
    badges[badge_count++] = (seL4_Word)1 << 6; /* unlock */
    badges[badge_count++] = (seL4_Word)1 << 6; /* malformed reply must wait */
    badges[badge_count++] = (seL4_Word)1 << 6; /* locked again then unlocked */
    simplex_wait_for_vault();
    assert(response_index == 4 && badge_index == 4 && waiting_logs == 1 && starting_logs == 1);
    return 0;
}
