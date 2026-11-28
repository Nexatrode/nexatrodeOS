/* SPDX-License-Identifier: Apache-2.0
 * Bounded local RAW-MWM packs and copy-on-write public region catalogue.
 * Checksums detect damage; locally supplied packs are not author-authenticated.
 */
#pragma once
#include "storage_format.h"
#include <string.h>
#ifdef MAP_LARGE_IMPORT
#include "paged_format.h"
#define MAP_IMPORT_INBOX (352ULL*1024*1024)
#define MAP_IMPORT_BANK_BYTES MAP_BUNDLE_LIMIT
#else
#define MAP_IMPORT_INBOX (160ULL*1024*1024)
#define MAP_IMPORT_BANK_BYTES MAP_FILE_LIMIT
#endif
#define MAP_IMPORT_CATALOG (2u*1024*1024)
#define MAP_IMPORT_EXTENT(r,b) (64ULL*1024*1024+((uint64_t)(r)*2+(b))*MAP_IMPORT_BANK_BYTES)
#define MAP_IMPORT_BASE_BANK 2u
struct map_local_region {uint8_t identity[16];char name[64];uint8_t digest[32];uint64_t length,version;uint32_t bank,present;};
struct map_local_pack {uint8_t identity[16];char name[64];uint8_t digest[32];uint64_t length,version;uint32_t region,kind;};
_Static_assert(sizeof(struct map_local_region)==136,"local catalogue ABI");
static inline bool map_local_name(const char *p){bool ended=false;for(unsigned i=0;i<64;i++){unsigned char c=p[i];if(!c)ended=true;else if(ended||c<32||c>126)return false;}return ended&&p[0];}
static inline bool map_local_identity(const uint8_t *p){unsigned any=0;for(unsigned i=0;i<16;i++)any|=p[i];return any!=0;}
static inline bool map_local_pack_read(const uint8_t *p,struct map_local_pack *pack){
    uint32_t format,r;uint8_t hash[32];uint64_t v,n;memcpy(&format,p+8,4);memcpy(&r,p+12,4);memcpy(&v,p+16,8);memcpy(&n,p+24,8);map_sha256(p,256,hash);
    unsigned kind=1;uint64_t limit=MAP_FILE_LIMIT;
#ifdef MAP_LARGE_IMPORT
    if(format==2&&!memcmp(p,"NXMRPK02",8)){kind=2;limit=MAP_BUNDLE_LIMIT;}
#endif
    if((kind==1&&(memcmp(p,"NXMRPK01",8)||format!=1||r==2))||r>=4||!v||v==UINT64_MAX||n<16||n>limit||memcmp(hash,p+256,32))return false;
    for(unsigned i=144;i<256;i++)if(p[i])return false;for(unsigned i=288;i<4096;i++)if(p[i])return false;
    memset(pack,0,sizeof(*pack));pack->region=r;pack->kind=kind;pack->version=v;pack->length=n;memcpy(pack->identity,p+32,16);memcpy(pack->name,p+48,64);memcpy(pack->digest,p+112,32);
    return map_local_identity(pack->identity)&&map_local_name(pack->name);
}
static inline bool map_local_catalog_read(const uint8_t *p,struct map_local_region *regions,uint64_t *generation){
    uint8_t h[32];uint64_t gen;map_sha256(p,1024,h);memcpy(&gen,p+8,8);
    if(memcmp(p,"NXMRCT01",8)||!gen||gen==UINT64_MAX||memcmp(h,p+1024,32))return false;
    for(unsigned i=16+4*sizeof(*regions);i<1024;i++)if(p[i])return false;for(unsigned i=1056;i<4096;i++)if(p[i])return false;
    memcpy(regions,p+16,4*sizeof(*regions));
    for(unsigned i=0;i<4;i++){struct map_local_region *r=&regions[i];if(r->present>
#ifdef MAP_LARGE_IMPORT
        2
#else
        1
#endif
        ||r->bank>2)return false;
        if(!r->present){struct map_local_region zero={0};if(memcmp(r,&zero,sizeof(zero)))return false;continue;}
        if(!map_local_identity(r->identity)||!map_local_name(r->name)||!r->version||r->version==UINT64_MAX||r->length<16||r->length>(
#ifdef MAP_LARGE_IMPORT
        r->present==2?MAP_BUNDLE_LIMIT:
#endif
        MAP_FILE_LIMIT))return false;
        if(i==2&&r->present==1&&r->bank!=2)return false;if(r->present==2&&r->bank==2)return false;if(i==3&&r->bank==2)return false;
        for(unsigned j=0;j<i;j++)if(regions[j].present&&!memcmp(regions[j].identity,r->identity,16))return false;
    }*generation=gen;return true;
}
static inline void map_local_catalog_write(uint8_t *p,const struct map_local_region *r,uint64_t generation){memset(p,0,4096);memcpy(p,"NXMRCT01",8);memcpy(p+8,&generation,8);memcpy(p+16,r,4*sizeof(*r));map_sha256(p,1024,p+1024);}
static inline bool map_local_update_allowed(const struct map_local_pack *p,const struct map_local_region *r){
    return !r->present||(!memcmp(p->identity,r->identity,16)&&!memcmp(p->name,r->name,64)&&p->version>r->version);
}
