/* SPDX-License-Identifier: MIT
 * Bounded, fenced modern VirtIO transport used by the native GPU experiment. */
#pragma once
static uintptr_t gpu;
static uint16_t submitted;
static uint64_t next_fence;
#define GPU_REQUEST 0x4000u
#define GPU_REQUEST_CAPACITY 0x4000u
#define GPU_BATCH_SLOTS 4u
#define GPU_BATCH_BASE 0x8000u
#define GPU_CURSOR_BASE 0x18000u
_Static_assert(GPU_BATCH_BASE+GPU_BATCH_SLOTS*GPU_REQUEST_CAPACITY==GPU_CURSOR_BASE,"command slots precede cursor");
_Static_assert(GPU_CURSOR_BASE+64u*88u*4u<=0x20000u,"cursor precedes scanout DMA");
static uint64_t command_wait_ticks;
static uint32_t command_count, synchronous_command_count;
static bool batch_started;
static uint16_t completed;
static unsigned pending_commands;
static bool batch_busy[GPU_BATCH_SLOTS];
static bool batch_fenced[GPU_BATCH_SLOTS];
static uint64_t batch_fences[GPU_BATCH_SLOTS];
struct descriptor { uint64_t address; uint32_t length; uint16_t flags,next; };
struct header { uint32_t type,flags; uint64_t fence; uint32_t context; uint8_t ring,padding[3]; };
_Static_assert(sizeof(struct header)==24,"VirtIO GPU header");
static void clear(void *p,size_t n) { volatile uint8_t *b=p;while(n--)*b++=0; }
#ifdef GPU_TRANSPORT_HOST_TEST
#define GPU_BARRIER() atomic_thread_fence(memory_order_seq_cst)
static uint64_t ticks(void) {return gpu_transport_test_ticks();}
static uint64_t frequency(void) {return 1000000;}
#else
#define GPU_BARRIER() asm volatile("dmb sy":::"memory")
static uint64_t ticks(void) { uint64_t n;asm volatile("mrs %0,cntpct_el0":"=r"(n));return n; }
static uint64_t frequency(void) { uint64_t n;asm volatile("mrs %0,cntfrq_el0":"=r"(n));return n; }
#endif
static void number(uint64_t n) { char b[24];unsigned i=0;do{b[i++]=(char)('0'+n%10);n/=10;}while(n);while(i)microkit_dbg_putc(b[--i]); }
static void field(const char *s,uint64_t n) {microkit_dbg_puts(s);number(n);microkit_dbg_puts("\n");}
static uint32_t read_reg(unsigned o) {return *(volatile uint32_t*)(gpu+o);}
static void write_reg(unsigned o,uint32_t n) {*(volatile uint32_t*)(gpu+o)=n;GPU_BARRIER();
#ifdef GPU_TRANSPORT_HOST_TEST
    if(o==0x50)gpu_transport_test_notify();
#endif
}
static void address(unsigned o,uint64_t a) {write_reg(o,(uint32_t)a);write_reg(o+4,a>>32);}
/* Every request's fixed fields fit in 128 bytes. Submit payload is completely
 * overwritten by its producer; do not clear 16 KiB of uncached DMA per call. */
static struct header *request(void) {clear((void*)(dma+GPU_REQUEST),128);return (void*)(dma+GPU_REQUEST);}
static struct header *command(uint32_t type,uint32_t context,unsigned bytes) {
    if(pending_commands||bytes>GPU_REQUEST_CAPACITY)return NULL;
    uint64_t expected_fence=++next_fence;
    struct header *q=(void*)(dma+GPU_REQUEST);q->type=type;q->flags=1;q->fence=expected_fence;q->context=context;
    /* Only the response header is consumed here. Clearing the entire 64 KiB
     * receive capacity for every tiny completion wastes uncached DMA writes.
     * A partial/stale header is rejected by length, fence and flag checks. */
    clear((void*)(dma+0x2000),sizeof(struct header));
    volatile struct descriptor *d=(void*)dma;
    d[0]=(struct descriptor){dma_paddr+GPU_REQUEST,bytes,1,1};
    d[1]=(struct descriptor){dma_paddr+0x2000,4096,2,0};
    volatile uint16_t *avail=(void*)(dma+0x100),*used=(void*)(dma+0x200);
    avail[2+submitted%8]=0;GPU_BARRIER();avail[1]=++submitted;
    write_reg(0x50,0);uint64_t wait_begin=ticks(),end=wait_begin+frequency()*3;
    while(used[1]!=submitted)if(ticks()>end){microkit_dbg_puts("DESKTOP_GPU|FAIL|queue timeout\n");return NULL;}
    command_wait_ticks+=ticks()-wait_begin;++command_count;++synchronous_command_count;
    completed=submitted;
    GPU_BARRIER();
    volatile uint32_t *entry=(void*)(dma+0x204+8*((submitted-1)%8));
    struct header *r=(void*)(dma+0x2000);
    if(entry[0]!=0 || entry[1]<sizeof(*r) || entry[1]>4096 || !(r->flags&1) || r->fence!=expected_fence){microkit_dbg_puts("DESKTOP_GPU|FAIL|completion validation\n");return NULL;}
    return r;
}
/* At most one frame is in flight. Each command has independent request,
 * response and descriptor storage until its fenced completion is consumed.
 * The three steady-frame commands fit in four descriptor pairs (queue size 8).
 * Kick once after enqueueing them, then validate every completion, including
 * out-of-order responses, before touching buffers or retiring any resource. */
/* Nonblocking completion consumption: an incomplete queue retains every lease
 * and returns to Microkit. IRQ/timer wakeups call this again without spinning. */
static void kick_batch(void) {
    if(pending_commands&&!batch_started){
        /* Queue order plus the final fence covers all earlier GPU work. Keep
         * every request lease until its response, and forbid buffer/texture
         * reuse or appending until the complete batch (including that fence)
         * retires. Extra GL fences flush the host unnecessarily. */
        uint64_t last=0;for(unsigned i=0;i<GPU_BATCH_SLOTS;++i)if(batch_busy[i]&&batch_fences[i]>last)last=batch_fences[i];
        for(unsigned i=0;i<GPU_BATCH_SLOTS;++i)if(batch_busy[i]){
            struct header *q=(void*)(dma+GPU_BATCH_BASE+i*GPU_REQUEST_CAPACITY);
            batch_fenced[i]=batch_fences[i]==last;q->flags=batch_fenced[i]?1:0;
        }
        /* Publish only after every request, fence policy and descriptor is
         * final. A device may inspect an already kicked queue concurrently. */
        GPU_BARRIER();batch_started=true;*(volatile uint16_t*)(dma+0x102)=submitted;GPU_BARRIER();write_reg(0x50,0);
    }
}
static int poll_batch(void) {
    if(!pending_commands)return 1;
    if(!batch_started)return -1;
    volatile uint16_t *used=(void*)(dma+0x200);
    uint16_t count=used[1];GPU_BARRIER();
    if((uint16_t)(count-completed)>pending_commands){microkit_dbg_puts("DESKTOP_GPU|FAIL|batch count\n");return -1;}
    while(completed!=count){
        volatile uint32_t *entry=(void*)(dma+0x204+8*(completed%8));
        unsigned id=entry[0],slot=id/2;
        if(id>=8||(id&1)||!batch_busy[slot]||entry[1]<sizeof(struct header)||entry[1]>64){microkit_dbg_puts("DESKTOP_GPU|FAIL|batch descriptor\n");return -1;}
        struct header *r=(void*)(dma+0x3000+slot*64);
        if(r->type!=0x1100||!!(r->flags&1)!=batch_fenced[slot]||(batch_fenced[slot]&&r->fence!=batch_fences[slot])){microkit_dbg_puts("DESKTOP_GPU|FAIL|batch fence\n");return -1;}
        batch_busy[slot]=false;--pending_commands;++completed;++command_count;
    }
    if(!pending_commands){batch_started=false;return 1;}
    return 0;
}
/* Diagnostic readback only. Normal frame drawing and uploads use kick/poll. */
static bool __attribute__((unused)) finish_batch(void) {
    if(!pending_commands)return true;
    uint64_t begin=ticks(),end=begin+frequency()*3;kick_batch();int result;
    while((result=poll_batch())==0)if(ticks()>end){microkit_dbg_puts("DESKTOP_GPU|FAIL|batch timeout\n");return false;}
    command_wait_ticks+=ticks()-begin;return result>0;
}
static bool defer_command(uint32_t type,uint32_t context,unsigned bytes) {
    if(bytes>GPU_REQUEST_CAPACITY||batch_started||pending_commands==GPU_BATCH_SLOTS)return false;
    unsigned slot=0;while(slot<GPU_BATCH_SLOTS&&batch_busy[slot])++slot;
    if(slot==GPU_BATCH_SLOTS)return false;
    uintptr_t offset=GPU_BATCH_BASE+slot*GPU_REQUEST_CAPACITY;
    uint8_t *from=(void*)(dma+GPU_REQUEST);volatile uint8_t *to=(void*)(dma+offset);
    for(unsigned i=0;i<bytes;++i)to[i]=from[i];
    uint64_t expected_fence=++next_fence;
    struct header *q=(void*)(dma+offset);q->type=type;q->flags=1;q->fence=expected_fence;q->context=context;
    clear((void*)(dma+0x3000+slot*64),sizeof(struct header));
    volatile struct descriptor *d=(void*)dma;
    d[slot*2]=(struct descriptor){dma_paddr+offset,bytes,1,slot*2+1};
    d[slot*2+1]=(struct descriptor){dma_paddr+0x3000+slot*64,64,2,0};
    batch_busy[slot]=true;batch_fences[slot]=expected_fence;++pending_commands;
    volatile uint16_t *avail=(void*)(dma+0x100);avail[2+submitted%8]=slot*2;
    ++submitted;return true;
}
