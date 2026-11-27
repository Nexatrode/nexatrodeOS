/* Exercise actual receive/wide-receive code with three distinct mapped pages. */
#include <stdio.h>
#define DESKTOP_BROWSER_NETWORK 1
#define DESKTOP_PRIVATE_TCP 1
#define NEXAWAL_UTM_WIDE_RX 1
#define NEXAWAL_UTM_RX_AHEAD 1
#define main unused_receive_tests
#include "haskell_tcp_receive_test.c"
#undef main

int main(void)
{
    static union { uint64_t align; uint8_t bytes[4096]; } agent, wallet, browser;
    haskell_tcp_page=(uintptr_t)&agent;
    wallet_tcp_page=(uintptr_t)&wallet;
    browser_tcp_page=(uintptr_t)&browser;
    memset(agent.bytes,0xa5,sizeof(agent.bytes));
    memset(wallet.bytes,0,sizeof(wallet.bytes));
    memset(browser.bytes,0,sizeof(browser.bytes));
    for(unsigned slot=0;slot<2;++slot)assert(desktop_tcp_slot(HASKELL_TCP_CHANNEL,slot)==(int)slot);
    for(unsigned slot=2;slot<6;++slot)assert(desktop_tcp_slot(HASKELL_TCP_CHANNEL,slot)==-1);
    assert(desktop_tcp_slot(HASKELL_TCP_CHANNEL,UINT32_MAX)==-1);
    assert(desktop_tcp_slot(WALLET_TCP_CHANNEL,UINT32_MAX)==2);
    assert(desktop_tcp_slot(WORKER_TCP_CHANNEL,0)==3);
    assert(desktop_tcp_slot(BROWSER_TCP_CHANNEL,0)==4);
    for(unsigned slot=2;slot<5;++slot){
        haskell_tcp_select(slot,true);
        volatile struct haskell_tcp_page *io=haskell_tcp_wire_page();
        uintptr_t expected=slot==4?browser_tcp_page:wallet_tcp_page+slot*sizeof(*io);
        assert((uintptr_t)io==expected);
        /* The previous shared-page implementation must not satisfy this. */
        assert((uintptr_t)io!=haskell_tcp_page+slot*sizeof(*io));
        sessions[slot].state=CONNECTED;sessions[slot].rx_length=3;sessions[slot].rx_head=0;
        memcpy(rx_buffer(&sessions[slot]),"abc",3);io->request_len=3;
        assert(haskell_tcp_recv()==HASKELL_TCP_DATA&&io->response_len==3);
        assert(!memcmp((void*)io->response,"abc",3));
        for(unsigned n=0;n<4096;++n)assert(agent.bytes[n]==0xa5);
    }
    haskell_tcp_select(2,true);
    sessions[2].rx_head=0;sessions[2].rx_length=2048;
    memset(rx_buffer(&sessions[2]),0x73,2048);
    haskell_tcp_wire_page()->request_len=2048;
    assert(haskell_tcp_recv_wide()==HASKELL_TCP_DATA);
    for(unsigned n=0;n<2048;++n)assert(wallet.bytes[2048+n]==0x73);
    for(unsigned n=0;n<4096;++n)assert(agent.bytes[n]==0xa5);
    puts("PASS endpoint-bound slots, distinct wallet/browser pages and private wide receive");
}
