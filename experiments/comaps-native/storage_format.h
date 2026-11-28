/* SPDX-License-Identifier: Apache-2.0
 * Public map pack, not a general filesystem or private-data vault.
 */
#pragma once
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#ifdef MAP_IMPORT
#define MAP_PACK_BYTES (192u*1024*1024)
#define MAP_REGION_COUNT 4u
#else
#define MAP_PACK_BYTES (64u*1024*1024)
#define MAP_REGION_COUNT 3u
#endif
#define MAP_PAGE_COUNT 65u
#define MAP_IO_DMA_BYTES 0x20000u
#define MAP_IO_PAYLOAD_OFFSET 0x1000u
#define MAP_IO_CHUNK (MAP_IO_DMA_BYTES-MAP_IO_PAYLOAD_OFFSET)
#define MAP_FILE_LIMIT (8u*1024*1024)
#define MAP_FILE_OFFSET(i) ((uint64_t)(i+1)*MAP_FILE_LIMIT)
#define MAP_STORAGE_MAGIC 0x4e584d53544f5231ULL
enum { MAP_STORE_BOOT=1, MAP_STORE_LOAD=2, MAP_STORE_COMMIT=3, MAP_PRIVATE_READ=4, MAP_PRIVATE_WRITE=5, MAP_STORE_PAGE=6, MAP_STORE_SEARCH=7, MAP_IMPORT_PROBE=8, MAP_IMPORT_READ=9, MAP_IMPORT_COMMIT=10 };
#define MAP_PRIVATE_OFFSET (1024u*1024u)
enum { MAP_STORE_OK=0, MAP_STORE_IO=1, MAP_STORE_CORRUPT=2, MAP_STORE_INVALID=3, MAP_STORE_CAPACITY=4, MAP_STORE_CANCELLED=5 };
/* Elapsed guest times include preemption. wait_us measures doorbell to
 * completion-handler entry, not bare disk time; pair with QEMU block stats. */
struct map_io_times {
    uint64_t requests,wait_us,max_wait_us,copy_us,max_copy_us,issue_us,max_issue_us;
    uint64_t dispatch_us,viewer_us,empty_irqs,bins[8],worst[4],ack_us,max_ack_us;
};
struct map_store_page {
    uint64_t sequence, command, region, length, generation, result, complete, chunk, progress, read_us, hash_us,requested_us,done_us;
    struct map_io_times io;
#ifdef MAP_IMPORT
    /* Public metadata only. Keep this handoff within its existing 4 KiB page. */
    uint8_t local_regions[4*136],local_pack[144];
    uint64_t catalog_generation,import_phase,cancel_sequence;
#endif
};
_Static_assert(sizeof(struct map_store_page)<=4096,"bounded storage handoff");
void map_sha256(const void *,size_t,uint8_t[32]);
/* Two independently checksummed sectors. Never overwrite the newest bank. */
static inline bool map_selection_read(const uint8_t *p,uint64_t *gen,unsigned *region) {
    uint64_t v[4];uint8_t digest[32];
    __builtin_memcpy(v,p,sizeof(v));map_sha256(p,32,digest);
    if(v[0]!=MAP_STORAGE_MAGIC||!v[1]||v[1]==UINT64_MAX||v[2]>=MAP_REGION_COUNT||v[3]!=~v[2]||__builtin_memcmp(p+32,digest,32))return false;
    *gen=v[1];*region=(unsigned)v[2];return true;
}
static inline void map_selection_write(uint8_t *p,uint64_t gen,unsigned region) {
    __builtin_memset(p,0,4096);uint64_t v[]={MAP_STORAGE_MAGIC,gen,region,~(uint64_t)region};
    __builtin_memcpy(p,v,sizeof(v));map_sha256(p,32,p+32);
}
