/* SPDX-License-Identifier: Apache-2.0
 * Single-inflight guest install: inactive extent -> readback -> data FLUSH ->
 * inactive catalogue sector -> catalogue FLUSH. Private banks are untouched.
 */
#pragma once
#include "import_format.h"
static struct map_local_region local_regions[4],local_candidate[4];
static struct map_local_pack import_pack;
static uint8_t local_journals[8192];
static uint64_t local_generation;
static unsigned local_bank,import_bank;
static bool import_ready,import_probed;
#ifdef MAP_LARGE_IMPORT
static unsigned bundle_segment;
static uint64_t bundle_peak;
#endif
static uint8_t import_digest[32];
static void local_publish(void){
    memcpy(page()->local_regions,local_regions,sizeof(local_regions));memcpy(page()->local_pack,&import_pack,sizeof(import_pack));page()->catalog_generation=local_generation;page()->import_phase=phase;
}
static bool import_cancelled(void){uint64_t cancel=__atomic_load_n(&page()->cancel_sequence,__ATOMIC_ACQUIRE);return cancel&&cancel==page()->sequence;}
static void import_write_next(void){
    size_t n=import_pack.length-offset;if(n>CHUNK)n=CHUNK;size_t aligned=(n+511)&~(size_t)511;
    memset((void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET),0,aligned);dma_write((void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET),(void*)(map_store_bytes+offset),n);
    page()->import_phase=11;
    if(!submit(1,MAP_IMPORT_EXTENT(import_pack.region,import_bank)+offset,aligned))done(MAP_STORE_IO);
}
static bool local_complete(void){
    /* Finish the current DMA first. The old catalogue remains authoritative
     * until phase 14 starts; beyond that point finish the durable commit. */
    if(phase>=10&&phase<=13&&import_cancelled()){import_ready=import_probed=false;done(MAP_STORE_CANCELLED);return true;}
    if(phase==8){
        dma_read(local_journals,(void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET),8192);local_generation=0;bool valid=false;
        for(unsigned i=0;i<2;i++){uint64_t g;if(map_local_catalog_read(local_journals+i*4096,local_candidate,&g)&&g>local_generation){local_generation=g;local_bank=i;memcpy(local_regions,local_candidate,sizeof(local_regions));valid=true;}}
        if(!valid){done(MAP_STORE_CORRUPT);return true;}
        phase=1;if(!submit(0,0,8192))done(MAP_STORE_IO);return true;
    }
    if(phase==10){
        uint8_t header[4096];dma_read(header,(void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET),4096);
        memset(&import_pack,0,sizeof(import_pack));import_ready=false;import_probed=false;
        if(!map_local_pack_read(header,&import_pack)||!map_local_update_allowed(&import_pack,&local_regions[import_pack.region])){done(MAP_STORE_INVALID);return true;}
        for(unsigned i=0;i<4;i++)if(i!=import_pack.region&&local_regions[i].present&&!memcmp(local_regions[i].identity,import_pack.identity,16)){done(MAP_STORE_INVALID);return true;}
        import_probed=true;region=import_pack.region;done(MAP_STORE_OK);return true;
    }
    if(phase==11){
        size_t n=import_pack.length-offset;if(n>amount)n=amount;offset+=n;page()->progress=offset;
        if(offset<import_pack.length){import_write_next();return true;}
        /* Read back into the same bounded public buffer before trusting DMA. */
        phase=12;page()->import_phase=12;offset=0;target_length=import_pack.length;target_offset=MAP_IMPORT_EXTENT(region,import_bank);read_next();return true;
    }
    if(phase==12){
        size_t n=target_length-offset;if(n>amount)n=amount;dma_read((void*)(map_store_bytes+offset),(void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET),n);offset+=n;page()->progress=offset;
        if(offset<target_length){read_next();return true;}
        uint8_t digest[32];map_sha256((void*)map_store_bytes,target_length,digest);
        if(memcmp(digest,import_pack.digest,32)){done(MAP_STORE_CORRUPT);return true;}
        phase=13;page()->import_phase=13;if(!submit(4,0,0))done(MAP_STORE_IO);return true;
    }
    if(phase==13){
        memcpy(local_candidate,local_regions,sizeof(local_regions));struct map_local_region *r=&local_candidate[region];memset(r,0,sizeof(*r));
        memcpy(r->identity,import_pack.identity,16);memcpy(r->name,import_pack.name,64);memcpy(r->digest,import_pack.digest,32);r->length=import_pack.length;r->version=import_pack.version;r->bank=import_bank;r->present=import_pack.kind;
        map_local_catalog_write(local_journals,local_candidate,local_generation+1);
        dma_write((void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET),local_journals,4096);phase=14;page()->import_phase=14;
        if(!submit(1,MAP_IMPORT_CATALOG+(1-local_bank)*4096,4096))done(MAP_STORE_IO);return true;
    }
    if(phase==14){phase=15;page()->import_phase=15;if(!submit(4,0,0))done(MAP_STORE_IO);return true;}
    if(phase==15){
        local_bank=1-local_bank;++local_generation;memcpy(local_regions,local_candidate,sizeof(local_regions));readable=true;readable_region=region;
        microkit_dbg_puts("MAPIMPORT|COMMITTED\n");done(MAP_STORE_OK);return true;
    }return false;
}
#ifdef MAP_FILES
static bool files_begin(void);
static void files_step(void);
#ifdef MAP_LARGE_IMPORT
static void bundle_start_segment(void);
#endif
#endif
static bool local_request(unsigned command){
    if(command!=MAP_IMPORT_COMMIT)import_ready=false;
    if(command==MAP_STORE_BOOT){phase=8;if(!submit(0,MAP_IMPORT_CATALOG,8192))done(MAP_STORE_IO);return true;}
    if(command>=MAP_IMPORT_PROBE&&command<=MAP_IMPORT_COMMIT&&!local_generation){done(MAP_STORE_CORRUPT);return true;}
    if(command==MAP_IMPORT_PROBE){region=0;
#ifdef MAP_FILES
        if(!files_begin())done(MAP_STORE_INVALID);return true;
#endif
        phase=10;if(!submit(0,MAP_IMPORT_INBOX,4096))done(MAP_STORE_IO);return true;}
    if(command==MAP_IMPORT_READ){
        if(!import_probed||!import_pack.version||!map_local_update_allowed(&import_pack,&local_regions[import_pack.region])){done(MAP_STORE_INVALID);return true;}
        region=import_pack.region;target_length=import_pack.length;
#ifdef MAP_FILES
#ifdef MAP_LARGE_IMPORT
        if(import_pack.kind==2){import_bank=local_regions[region].bank==0?1:0;bundle_segment=0;bundle_peak=0;page()->peak_buffered=0;bundle_start_segment();return true;}
#endif
        phase=17;page()->import_phase=17;offset=0;read_start=us();files_step();return true;
#endif
        target_offset=MAP_IMPORT_INBOX+4096;memcpy(import_digest,import_pack.digest,32);target_digest=import_digest;
        phase=2;offset=0;read_start=us();read_next();return true;
    }
    if(command==MAP_IMPORT_COMMIT){
        if(!import_ready||local_generation>=UINT64_MAX-1||!map_local_update_allowed(&import_pack,&local_regions[import_pack.region])){done(MAP_STORE_INVALID);return true;}
        import_ready=false;region=import_pack.region;
#ifdef MAP_LARGE_IMPORT
        if(import_pack.kind==2){phase=13;page()->import_phase=13;if(!submit(4,0,0))done(MAP_STORE_IO);return true;}
#endif
        import_bank=local_regions[region].bank==0?1:0;phase=11;offset=0;page()->progress=0;readable=false;import_write_next();return true;
    }
    return false;
}
