/* SPDX-License-Identifier: Apache-2.0
 * Streaming install. The one 8 MiB workspace is reused for each segment and
 * its readback; no allocation or whole-container plaintext buffer exists.
 */
#include "paged_format.h"
#include "../files-picker/protocol.h"
#include "spatial.h"
#include "search.h"
static struct map_directory bundle_directory,runtime_directory;
static uint8_t bundle_directory_raw[MAP_DIRECTORY_BYTES];
static unsigned runtime_region=MAP_REGION_COUNT;
static uint64_t bundle_at;
static const uint8_t *bundle_hash;
static void files_step(void),files_release(void);
static bool files_read(uint64_t,void*,unsigned);
static bool bundle_validate(unsigned which){
 const uint8_t*p=(void*)map_store_bytes;
 if(!which){struct map_directory d;return map_directory_read(p,&d)&&d.length==import_pack.length&&d.version==import_pack.version&&!memcmp(d.identity,import_pack.identity,16);}
 if(which<=MAP_PAGE_COUNT){
  if(!map_page_append(NULL,p,target_length,NULL))return false;
  const double*b=bundle_directory.segments[which-1].bounds;size_t at=16;uint32_t count;memcpy(&count,p+4,4);
  for(unsigned i=0;i<count;i++){struct map_feature f;memcpy(&f,p+at,sizeof(f));at+=sizeof(f)+(size_t)f.count*sizeof(struct map_point);double ox=bundle_directory.origin[0],oy=bundle_directory.origin[1];
   if(f.minx+ox<b[0]-1e-7||f.miny+oy<b[1]-1e-7||f.maxx+ox>b[2]+1e-7||f.maxy+oy>b[3]+1e-7)return false;
  }return true;
 }
 double bounds[]={bundle_directory.bounds[0]-bundle_directory.origin[0]-.001,bundle_directory.bounds[1]-bundle_directory.origin[1]-.001,bundle_directory.bounds[2]-bundle_directory.origin[0]+.001,bundle_directory.bounds[3]-bundle_directory.origin[1]+.001};unsigned count;
 return map_search_catalogue_read(NULL,&count,p,target_length,bounds);
}
static void bundle_write_next(void){
 size_t n=target_length-offset;if(n>CHUNK)n=CHUNK;size_t aligned=(n+511)&~(size_t)511;
 memset((void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET),0,aligned);dma_write((void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET),(void*)(map_store_bytes+offset),n);
 if(!submit(1,MAP_IMPORT_EXTENT(region,import_bank)+bundle_at+offset,aligned))done(MAP_STORE_IO);
}
static void bundle_start_segment(void){
 if(!bundle_segment){bundle_at=0;target_length=MAP_DIRECTORY_BYTES;bundle_hash=import_pack.digest;}
 else{const struct map_segment*s=&bundle_directory.segments[bundle_segment-1];bundle_at=s->offset;target_length=s->length;bundle_hash=s->digest;}
 if(target_length>bundle_peak)bundle_peak=target_length;page()->peak_buffered=bundle_peak;
 offset=0;phase=30;page()->import_phase=phase;page()->progress=bundle_at;files_step();
}
static void bundle_source_step(void){
 if(import_cancelled()){import_ready=import_probed=false;done(MAP_STORE_CANCELLED);return;}
 size_t n=target_length-offset;unsigned boundary=FILES_CHUNK-(unsigned)((4096+bundle_at+offset)%FILES_CHUNK);if(n>boundary)n=boundary;
 if(!files_read(4096+bundle_at+offset,(void*)(map_store_bytes+offset),n)){done(MAP_STORE_INVALID);return;}offset+=n;page()->progress=bundle_at+offset;
 if(offset<target_length)return;
 uint8_t hash[32];map_sha256((void*)map_store_bytes,target_length,hash);
 if(memcmp(hash,bundle_hash,32)||!bundle_validate(bundle_segment)){done(MAP_STORE_CORRUPT);return;}
 phase=31;page()->import_phase=phase;offset=0;bundle_write_next();
}
static bool bundle_complete(void){
 if(phase!=31&&phase!=32)return false;
 if(import_cancelled()){import_ready=import_probed=false;done(MAP_STORE_CANCELLED);return true;}
 size_t n=target_length-offset;if(n>amount)n=amount;
 if(phase==31){offset+=n;page()->progress=bundle_at+offset;if(offset<target_length){bundle_write_next();return true;}phase=32;page()->import_phase=phase;offset=0;target_offset=MAP_IMPORT_EXTENT(region,import_bank)+bundle_at;read_next();return true;}
 dma_read((void*)(map_store_bytes+offset),(void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET),n);offset+=n;page()->progress=bundle_at+offset;
 if(offset<target_length){read_next();return true;}
 uint8_t hash[32];map_sha256((void*)map_store_bytes,target_length,hash);if(memcmp(hash,bundle_hash,32)){done(MAP_STORE_CORRUPT);return true;}
 if(++bundle_segment<=MAP_PAGE_COUNT+1){bundle_start_segment();return true;}
 files_release();import_ready=true;target_length=import_pack.length;page()->progress=target_length;microkit_dbg_puts("MAPBUNDLE|STAGED|all pages and search verified\n");done(MAP_STORE_OK);return true;
}
static bool bundle_directory_loaded(void){
 if(page()->command!=MAP_STORE_DIRECTORY)return true;
 uint8_t hash[32];map_sha256((void*)map_store_bytes,MAP_DIRECTORY_BYTES,hash);
 if(memcmp(hash,local_regions[region].digest,32)||!map_directory_read((void*)map_store_bytes,&runtime_directory)||runtime_directory.length!=local_regions[region].length||runtime_directory.version!=local_regions[region].version||memcmp(runtime_directory.identity,local_regions[region].identity,16))return false;
 runtime_region=region;return true;
}
static bool bundle_request(unsigned command){
 if(command==MAP_STORE_DIRECTORY){
  if(page()->region>=MAP_REGION_COUNT||local_regions[page()->region].present!=2){done(MAP_STORE_INVALID);return true;}
  region=page()->region;runtime_region=MAP_REGION_COUNT;target_length=MAP_DIRECTORY_BYTES;target_offset=MAP_IMPORT_EXTENT(region,local_regions[region].bank);target_digest=local_regions[region].digest;phase=2;offset=0;read_start=us();read_next();return true;
 }
 return false;
}
