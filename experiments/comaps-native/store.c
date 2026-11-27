/* SPDX-License-Identifier: Apache-2.0
 * Dedicated public-map disk. One IRQ-driven VirtIO request in flight; no vault,
 * network, host service or UI-thread disk waits. Only the two pinned extents
 * are readable and only the two selection sectors are writable by default.
 * MAP_PRIVATE adds only two fixed ciphertext banks, never private plaintext.
 */
#include <microkit.h>
#include <stdatomic.h>
#include <string.h>
#include "storage_format.h"
#include "regions_generated.h"
uintptr_t map_disk_regs,map_disk_dma,map_store_control,map_store_bytes;
#ifdef MAP_PRIVATE
uintptr_t map_private_cipher;
static bool private_operation;
static unsigned private_bank;
#endif
#define DMA_PA 0x5f880000ULL
#define CHUNK 65536u
struct descriptor {uint64_t address;uint32_t length;uint16_t flags,next;};
static uint16_t avail,used;
static bool online,busy;
static unsigned phase,region,bank;
static uint64_t sequence,generation,pending_generation;
static size_t offset,amount;
static bool readable;
static unsigned readable_region;
static uint8_t journals[8192];
/* Device/uncached DMA mappings cannot use libc's unaligned tail loads. In
 * particular San Marino's final chunk is not a multiple of eight bytes. */
static void dma_read(void *out,const void *in,size_t n) {
    uint8_t *d=out;const volatile uint8_t *s=in;for(size_t i=0;i<n;i++)d[i]=s[i];
}
static void dma_write(void *out,const void *in,size_t n) {
    volatile uint8_t *d=out;const uint8_t *s=in;for(size_t i=0;i<n;i++)d[i]=s[i];
}
static uint32_t reg(unsigned n){return *(volatile uint32_t*)(map_disk_regs+n);}
static void set(unsigned n,uint32_t v){*(volatile uint32_t*)(map_disk_regs+n)=v;}
static void address(unsigned n,uint64_t p){set(n,(uint32_t)p);set(n+4,(uint32_t)(p>>32));}
static struct map_store_page *page(void){return (void*)map_store_control;}
static void done(unsigned error) {
    struct map_store_page *p=page();p->region=region;p->generation=generation;
    p->result=error;p->length=error?0:map_regions[region].length;
#ifdef MAP_PRIVATE
    if(private_operation){p->region=private_bank;p->length=error?0:8192;private_operation=false;}
#endif
    __atomic_store_n(&p->complete,sequence,__ATOMIC_RELEASE);busy=false;microkit_notify(1);
}
static bool submit(unsigned code,uint64_t byte,size_t size) {
    if(!online||size>CHUNK||byte>MAP_PACK_BYTES||size>MAP_PACK_BYTES-byte||byte%512||size%512)return false;
    uint8_t *dma=(void*)map_disk_dma;uint64_t sector=byte/512;
    memcpy(dma+0x300,&code,4);memset(dma+0x304,0,4);memcpy(dma+0x308,&sector,8);dma[0x310]=255;
    struct descriptor *desc=(void*)dma;
    desc[0]=(struct descriptor){DMA_PA+0x300,16,1,code==4?2:1};
    desc[1]=(struct descriptor){DMA_PA+0x1000,(uint32_t)size,(uint16_t)(1|(code==0?2:0)),2};
    desc[2]=(struct descriptor){DMA_PA+0x310,1,2,0};
    volatile uint16_t *a=(void*)(dma+0x100);a[2+avail%8]=0;
    atomic_thread_fence(memory_order_seq_cst);a[1]=++avail;atomic_thread_fence(memory_order_seq_cst);set(0x50,0);
    amount=size;return true;
}
static void read_next(void) {
    size_t remain=map_regions[region].length-offset;
    size_t n=remain<CHUNK?((remain+511)&~(size_t)511):CHUNK;
    if(!submit(0,MAP_FILE_OFFSET(region)+offset,n))done(MAP_STORE_IO);
}
static void complete(void) {
    uint8_t *dma=(void*)map_disk_dma;volatile uint16_t *u=(void*)(dma+0x200);
    uint16_t next=u[1];if(next==used)return;
    atomic_thread_fence(memory_order_seq_cst);
    uint32_t *entry=(void*)(dma+0x204+(used%8)*8);
    unsigned expected=(phase==1||phase==2||phase==5)?(unsigned)amount+1:1;
    if(!busy||(uint16_t)(next-used)!=1||entry[0]!=0||entry[1]!=expected||dma[0x310]!=0){online=false;if(busy)done(MAP_STORE_IO);return;}
    used=next;
#ifdef MAP_PRIVATE
    if(phase==5){dma_read((void*)map_private_cipher,dma+0x1000,8192);done(MAP_STORE_OK);return;}
    if(phase==6){phase=7;if(!submit(4,0,0))done(MAP_STORE_IO);return;}
    if(phase==7){done(MAP_STORE_OK);return;}
#endif
    if(phase==1){
        dma_read(journals,dma+0x1000,8192);generation=0;bank=1;region=0;
        for(unsigned i=0;i<2;i++){uint64_t gen;unsigned r;if(map_selection_read(journals+i*4096,&gen,&r)&&gen>generation){generation=gen;region=r;bank=i;}}
        done(MAP_STORE_OK);
    }else if(phase==2){
        size_t n=map_regions[region].length-offset;if(n>amount)n=amount;
        dma_read((void*)(map_store_bytes+offset),dma+0x1000,n);offset+=n;
        if(offset<map_regions[region].length){read_next();return;}
        uint8_t digest[32];map_sha256((void*)map_store_bytes,offset,digest);
        if(memcmp(digest,map_regions[region].digest,32)){done(MAP_STORE_CORRUPT);return;}
        readable=true;readable_region=region;done(MAP_STORE_OK);
    }else if(phase==3){phase=4;if(!submit(4,0,0))done(MAP_STORE_IO);}
    else if(phase==4){bank=1-bank;generation=pending_generation;done(MAP_STORE_OK);}
}
void init(void) {
    if(reg(0)!=0x74726976||reg(4)!=2||reg(8)!=2)return;
    set(0x70,0);if(reg(0x70))return;set(0x70,3);
    set(0x14,0);if(!(reg(0x10)&(1u<<9)))return; /* Require durable FLUSH. */
    set(0x24,0);set(0x20,1u<<9);set(0x14,1);if(!(reg(0x10)&1))return;
    set(0x24,1);set(0x20,1);set(0x70,11);if(!(reg(0x70)&8))return;
    uint64_t capacity=(uint64_t)reg(0x104)<<32|reg(0x100);if(capacity<MAP_PACK_BYTES/512)return;
    set(0x30,0);if(reg(0x34)<8||reg(0x44))return;
    memset((void*)map_disk_dma,0,0x20000);set(0x38,8);
    address(0x80,DMA_PA);address(0x90,DMA_PA+0x100);address(0xa0,DMA_PA+0x200);set(0x44,1);set(0x70,15);online=true;
    microkit_dbg_puts("MAPSTORE|READY|dedicated public disk\n");
}
void notified(microkit_channel channel) {
    if(channel==0){uint32_t irq=reg(0x60);set(0x64,irq);complete();microkit_irq_ack(0);return;}
    if(channel!=1||busy)return;
    struct map_store_page *p=page();uint64_t next=__atomic_load_n(&p->sequence,__ATOMIC_ACQUIRE);
    if(!next||next==sequence)return;sequence=next;busy=true;
    if(!online){region=0;done(MAP_STORE_IO);return;}
#ifdef MAP_PRIVATE
    if(p->command==MAP_PRIVATE_READ){private_operation=true;private_bank=0;phase=5;if(!submit(0,MAP_PRIVATE_OFFSET,8192))done(MAP_STORE_IO);return;}
    if(p->command==MAP_PRIVATE_WRITE){private_operation=true;private_bank=p->region;region=0;
        if(private_bank>1){done(MAP_STORE_INVALID);return;}
        phase=6;dma_write((void*)(map_disk_dma+0x1000),(void*)(map_private_cipher+private_bank*4096),4096);
        if(!submit(1,MAP_PRIVATE_OFFSET+private_bank*4096,4096))done(MAP_STORE_IO);return;}
#endif
    if(p->command==MAP_STORE_BOOT){phase=1;if(!submit(0,0,8192))done(MAP_STORE_IO);return;}
    if(p->region>1){region=0;done(MAP_STORE_INVALID);return;}region=(unsigned)p->region;
    if(p->command==MAP_STORE_LOAD){readable=false;phase=2;offset=0;read_next();}
    else if(p->command==MAP_STORE_COMMIT&&readable&&region==readable_region&&generation<UINT64_MAX-1){
        phase=3;pending_generation=generation+1;map_selection_write(journals,pending_generation,region);
        dma_write((void*)(map_disk_dma+0x1000),journals,4096);
        if(!submit(1,(1-bank)*4096,4096))done(MAP_STORE_IO);
    }else done(MAP_STORE_INVALID);
}
