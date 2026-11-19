/* Independent DNS state for the network service's bounded client slots. */
#pragma once
#include <stdbool.h>
#include <stdint.h>
#include "haskell_tcp_protocol.h"

#define DNS_SLOT_COUNT HASKELL_TCP_SESSIONS

enum dns_slot_state { DNS_SLOT_IDLE, DNS_SLOT_PENDING, DNS_SLOT_DONE, DNS_SLOT_FAILED };
struct dns_slot {
    enum dns_slot_state state;
    char name[HASKELL_TCP_MAX + 1];
    uint8_t address[4];
};
struct dns_slot_table { struct dns_slot slots[DNS_SLOT_COUNT]; };

static inline bool dns_slot_begin(struct dns_slot_table *table, uint32_t index)
{
    if (index >= DNS_SLOT_COUNT || table->slots[index].state == DNS_SLOT_PENDING) return false;
    table->slots[index].state = DNS_SLOT_PENDING;
    return true;
}

static inline void dns_slot_complete(struct dns_slot_table *table, uint32_t index, const uint8_t *address)
{
    if (index >= DNS_SLOT_COUNT || table->slots[index].state != DNS_SLOT_PENDING) return;
    if (address == 0) { table->slots[index].state = DNS_SLOT_FAILED; return; }
    for (unsigned i = 0; i < 4; ++i) table->slots[index].address[i] = address[i];
    table->slots[index].state = DNS_SLOT_DONE;
}
