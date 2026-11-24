/* Exercise the actual TCP/DNS lifecycle implementation, including stale DNS
 * completions from a locked session. Existing receive mocks keep sockets local. */
#include <stdio.h>
#define SIMPLEX_LOCK_LIFECYCLE 1
#define main receive_tests_main
#include "haskell_tcp_receive_test.c"
#undef main

int main(void)
{
    static union { struct haskell_tcp_page pages[4]; uint8_t raw[4096]; } mapping;
    haskell_tcp_page=(uintptr_t)&mapping;
    assert(!haskell_simplex_network_active());
    assert(!haskell_simplex_network_transition(true,1));
    assert(!haskell_simplex_network_transition(false,0));
    for (unsigned n=0;n<2;++n) {
        sessions[n].pcb=&pcb;sessions[n].state=CONNECTED;sessions[n].rx_length=64;
        memset(rx_buffer(&sessions[n]),0x79,RX_CAPACITY);
        memset(&dns_slots.slots[n],0x3f,sizeof(dns_slots.slots[n]));
    }
    memset(mapping.raw,0x6e,sizeof(mapping.raw));
    assert(!haskell_simplex_network_cleared());
    assert(haskell_simplex_network_transition(false,1));
    assert(haskell_simplex_network_cleared());
    for(size_t i=0;i<sizeof(mapping.raw);++i) assert(mapping.raw[i]==0);
    assert(haskell_simplex_network_transition(true,1));
    assert(haskell_simplex_network_active());
    assert(!haskell_simplex_network_transition(true,1));
    uintptr_t stale=(dns_generations[0]<<3)|1;
    assert(haskell_simplex_network_transition(false,2));
    assert(!haskell_simplex_network_transition(true,1));
    assert(!haskell_simplex_network_transition(false,1));
    assert(!haskell_simplex_network_active());
    assert(haskell_simplex_network_transition(true,2));
    assert(dns_slot_begin(&dns_slots,0));
    uintptr_t fresh=(++dns_generations[0]<<3)|1;
    ip_addr_t addr; IP_ADDR4(&addr,192,0,2,8);
    dns_found("old",&addr,(void*)stale);
    assert(dns_slots.slots[0].state==DNS_SLOT_PENDING);
    dns_found("new",&addr,(void*)fresh);
    assert(dns_slots.slots[0].state==DNS_SLOT_DONE);
    assert(dns_slots.slots[0].address[0]==192 && dns_slots.slots[0].address[3]==8);
    assert(haskell_simplex_network_transition(false,3));
    assert(haskell_simplex_network_cleared() && !haskell_simplex_network_active());
    dns_found("late",&addr,(void*)fresh);
    assert(haskell_simplex_network_cleared());
    puts("PASS SimpleX network epoch fencing, socket abort, buffer erasure and stale DNS completion");
    return 0;
}
