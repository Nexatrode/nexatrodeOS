/* Actual TCP lifecycle across every private desktop endpoint, including ahead RX. */
#include <stdio.h>
#define DESKTOP_BROWSER_NETWORK 1
#define DESKTOP_PRIVATE_TCP 1
#define SIMPLEX_LOCK_LIFECYCLE 1
#define DESKTOP_LOCK_NETWORK 1
#define NEXAWAL_UTM_WIDE_RX 1
#define NEXAWAL_UTM_RX_NOTIFY 1
#define NEXAWAL_UTM_RX_AHEAD 1
#define main unused_receive_tests
#include "haskell_tcp_receive_test.c"
#undef main
static void zero(const void *p,size_t n) {const uint8_t *b=p;for(size_t i=0;i<n;++i)assert(!b[i]);}
int main(void)
{
    static uint64_t agent[512],wallet[512],browser[512];
    haskell_tcp_page=(uintptr_t)agent;wallet_tcp_page=(uintptr_t)wallet;browser_tcp_page=(uintptr_t)browser;
    unsigned channels[]={HASKELL_TCP_CHANNEL,WALLET_TCP_CHANNEL,SHELL_TCP_CHANNEL,WORKER_TCP_CHANNEL,BROWSER_TCP_CHANNEL};
    for(unsigned i=0;i<5;++i){assert(!desktop_tcp_permitted(channels[i],false));assert(desktop_tcp_permitted(channels[i],true));}
    assert(desktop_tcp_permitted(SIMPLEX_NET_MANAGER_CHANNEL,false));
    assert(!haskell_simplex_network_transition(true,1));
    for(unsigned round=1;round<=4;++round){
        for(unsigned slot=0;slot<HASKELL_TCP_SESSIONS;++slot){
            sessions[slot].pcb=&pcb;sessions[slot].state=CONNECTED;sessions[slot].rx_length=42;sessions[slot].rx_head=123;sessions[slot].rx_waiting=true;
            memset(rx_buffer(&sessions[slot]),0x73,RX_CAPACITY+ahead_limit(&sessions[slot]));
            memset(&dns_slots.slots[slot],0x64,sizeof(dns_slots.slots[slot]));
        }
        memset(agent,0x62,sizeof(agent));memset(wallet,0x63,sizeof(wallet));memset(browser,0x64,sizeof(browser));
        assert(haskell_simplex_network_transition(false,round));
        assert(haskell_simplex_network_cleared()&&!haskell_simplex_network_active());
        zero(agent,sizeof(agent));zero(wallet,sizeof(wallet));zero(browser,sizeof(browser));
        for(unsigned slot=0;slot<HASKELL_TCP_SESSIONS;++slot)zero(&sessions[slot],sizeof(sessions[slot]));
        assert(!haskell_simplex_network_transition(true,round+1));
        assert(haskell_simplex_network_transition(true,round));
        assert(haskell_simplex_network_active());
        assert(!haskell_simplex_network_transition(true,round));
    }
    puts("PASS all five desktop TCP slots: endpoint gates, epoch fencing, abort, private pages and full receive-ahead erasure");
}
