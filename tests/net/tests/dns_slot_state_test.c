#include <assert.h>
#include <stdint.h>
#include "dns_slot_state.h"

int main(void) {
    struct dns_slot_table table = {0};
    uint8_t first[4] = {1, 2, 3, 4}, second[4] = {5, 6, 7, 8};
    assert(dns_slot_begin(&table, 0));
    assert(dns_slot_begin(&table, 2));
    assert(!dns_slot_begin(&table, 0));
    assert(!dns_slot_begin(&table, DNS_SLOT_COUNT));
    dns_slot_complete(&table, 2, second);
    assert(table.slots[0].state == DNS_SLOT_PENDING);
    assert(table.slots[2].state == DNS_SLOT_DONE && table.slots[2].address[0] == 5);
    dns_slot_complete(&table, 0, first);
    assert(table.slots[0].state == DNS_SLOT_DONE && table.slots[0].address[0] == 1);
    assert(dns_slot_begin(&table, 0));
    dns_slot_complete(&table, 0, 0);
    assert(table.slots[0].state == DNS_SLOT_FAILED);
    assert(table.slots[2].state == DNS_SLOT_DONE && table.slots[2].address[0] == 5);
    return 0;
}
