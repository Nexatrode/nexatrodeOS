/* Exercise the production queue with a fake device: independent DMA lifetimes,
 * out-of-order completion, wrapping ring indices and malformed responses. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdatomic.h>
#include <stdio.h>
#include <string.h>
static uintptr_t dma,dma_paddr;
static uint64_t clock_ticks;
static uint64_t gpu_transport_test_ticks(void) {return ++clock_ticks;}
static void gpu_transport_test_notify(void);
static void microkit_dbg_puts(const char *s) {(void)s;}
static void microkit_dbg_putc(char c) {(void)c;}
#define GPU_TRANSPORT_HOST_TEST
#include "../experiments/desktop-gpu/transport.h"
static uint8_t memory[0x20000],regs[4096];
static uint16_t seen;
static unsigned kicks,bad;
static bool delay_completion,hold_fenced;
static unsigned held_id;
static void complete_id(unsigned id) {
        struct descriptor *d=(void*)dma;volatile uint16_t *u=(void*)(dma+0x200);assert(id<8&&!(id&1));
        struct header *q=(void*)(uintptr_t)d[id].address;
        assert(d[id].length>=24&&d[id].length<=GPU_REQUEST_CAPACITY&&d[id].flags==1&&d[id].next==id+1);
        assert(q->type==0x207||q->type==0x103||q->type==0x104);
        if(q->type==0x207)assert(((uint32_t*)(q+1))[0]==0x12345678); /* Not later staging bytes. */
        struct header *r=(void*)(uintptr_t)d[id+1].address;
        *r=(struct header){.type=0x1100,.flags=q->flags&1,.fence=(q->flags&1)?q->fence:0xa5a5a5a5};
        uint32_t *entry=(void*)(dma+0x204+8*(u[1]%8));entry[0]=id;entry[1]=sizeof(*r);
        if(bad==1)++r->fence;
        if(bad==2)entry[0]=0xffffffff;
        if(bad==3)entry[1]=8;
        if(bad==4)r->flags=0;
        if(bad==5)r->type=0x1200;
        ++u[1];
}
static void complete_device_commands(void) {
    volatile uint16_t *a=(void*)(dma+0x100),*u=(void*)(dma+0x200);
    struct descriptor *d=(void*)dma;unsigned ids[4],n=0;
    while(seen!=a[1]){assert(n<4);ids[n++]=a[2+seen%8];++seen;}
    clock_ticks+=100;
    while(n){ /* Deliberately complete descriptor chains in reverse order. */
        unsigned id=ids[--n];struct header *q=(void*)(uintptr_t)d[id].address;
        if(hold_fenced&&(q->flags&1)){held_id=id;continue;}
        complete_id(id);
    }
    if(bad==6)++u[1];
}
static void gpu_transport_test_notify(void) {
    ++kicks;if(!delay_completion)complete_device_commands();
}
static void reset(void) {
    memset(memory,0,sizeof(memory));memset(regs,0,sizeof(regs));
    dma=dma_paddr=(uintptr_t)memory;gpu=(uintptr_t)regs;submitted=completed=seen=0;
    pending_commands=kicks=bad=0;next_fence=0;batch_started=delay_completion=hold_fenced=false;memset(batch_busy,0,sizeof(batch_busy));
    memset(memory+GPU_CURSOR_BASE,0xa5,64*88*4);
}
static bool draw(void) {
    request();*(uint32_t*)(dma+GPU_REQUEST+24)=0x12345678;
    return defer_command(0x207,1,64);
}
int main(void) {
    reset();assert(draw());request();assert(defer_command(0x103,0,48));request();assert(defer_command(0x104,0,48));
    assert(pending_commands==3&&kicks==0);memset(memory+GPU_REQUEST,0xee,GPU_REQUEST_CAPACITY);
    assert(*(uint16_t*)(dma+0x102)==0); /* Device cannot see unfinished requests. */
    assert(finish_batch()&&kicks==1&&!pending_commands&&completed==submitted);
    for(unsigned i=0;i<4;++i)assert(!batch_busy[i]);
    reset();for(unsigned i=0;i<4;++i)assert(draw());assert(!draw());assert(kicks==0&&pending_commands==4);
    assert(finish_batch()&&kicks==1);assert(draw());assert(finish_batch()&&kicks==2);
    assert(!defer_command(0x207,1,GPU_REQUEST_CAPACITY+1));
    reset();delay_completion=true;assert(draw());kick_batch();assert(poll_batch()==0&&pending_commands==1&&batch_busy[0]);
    uint64_t before=clock_ticks;for(unsigned i=0;i<1000;++i)assert(poll_batch()==0);
    assert(clock_ticks==before); /* Poll never spins or reads a clock. */
    assert(!draw()); /* No append or buffer reuse after submission. */
    complete_device_commands();assert(poll_batch()==1&&!pending_commands&&!batch_busy[0]);
    reset();hold_fenced=true;assert(draw());request();assert(defer_command(0x103,0,48));request();assert(defer_command(0x104,0,48));
    kick_batch();assert(poll_batch()==0&&pending_commands==1&&batch_started);assert(!draw());
    assert(!batch_fenced[0]&&!batch_fenced[1]&&batch_fenced[2]);
    complete_id(held_id);assert(poll_batch()==1&&!pending_commands&&!batch_started);
    reset();next_fence=65534;
    for(unsigned i=0;i<22000;++i){assert(draw());request();assert(defer_command(0x103,0,48));request();assert(defer_command(0x104,0,48));assert(finish_batch());}
    assert(next_fence==131534&&completed==submitted&&submitted==(uint16_t)66000);
    for(unsigned i=GPU_CURSOR_BASE;i<GPU_CURSOR_BASE+64*88*4;++i)assert(memory[i]==0xa5);
    request();struct header *r=command(0x103,0,48);assert(r&&r->fence==131535); /* Sync command after a batch. */
    for(unsigned failure=1;failure<=6;++failure){reset();assert(draw());bad=failure;assert(!finish_batch());}
    puts("PASS nonblocking delayed GPU completion, bounded leases, reversed completions, ring/fence wrap, sync handoff and six invalid response controls");
}
