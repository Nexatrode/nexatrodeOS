/* SPDX-License-Identifier: Apache-2.0
 * Public map pack, not a general filesystem or private-data vault.
 */
#pragma once
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#define MAP_PACK_BYTES (32u*1024*1024)
#define MAP_FILE_LIMIT (8u*1024*1024)
#define MAP_FILE_OFFSET(i) ((uint64_t)(i+1)*MAP_FILE_LIMIT)
#define MAP_STORAGE_MAGIC 0x4e584d53544f5231ULL
enum { MAP_STORE_BOOT=1, MAP_STORE_LOAD=2, MAP_STORE_COMMIT=3, MAP_PRIVATE_READ=4, MAP_PRIVATE_WRITE=5 };
#define MAP_PRIVATE_OFFSET (1024u*1024u)
enum { MAP_STORE_OK=0, MAP_STORE_IO=1, MAP_STORE_CORRUPT=2, MAP_STORE_INVALID=3 };
struct map_store_page {
    uint64_t sequence, command, region, length, generation, result, complete;
};
void map_sha256(const void *,size_t,uint8_t[32]);
/* Two independently checksummed sectors. Never overwrite the newest bank. */
static inline bool map_selection_read(const uint8_t *p,uint64_t *gen,unsigned *region) {
    uint64_t v[4];uint8_t digest[32];
    __builtin_memcpy(v,p,sizeof(v));map_sha256(p,32,digest);
    if(v[0]!=MAP_STORAGE_MAGIC||!v[1]||v[1]==UINT64_MAX||v[2]>1||v[3]!=~v[2]||__builtin_memcmp(p+32,digest,32))return false;
    *gen=v[1];*region=(unsigned)v[2];return true;
}
static inline void map_selection_write(uint8_t *p,uint64_t gen,unsigned region) {
    __builtin_memset(p,0,4096);uint64_t v[]={MAP_STORAGE_MAGIC,gen,region,~(uint64_t)region};
    __builtin_memcpy(p,v,sizeof(v));map_sha256(p,32,p+32);
}
