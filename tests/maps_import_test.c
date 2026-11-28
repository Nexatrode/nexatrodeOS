/* Exercise the guest import state machine with deterministic I/O completions. */
#define MAP_IMPORT 1
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include "../experiments/comaps-native/import_format.h"
#define CHUNK MAP_IO_CHUNK
static unsigned phase,region,amount;
static size_t offset,target_length;
static uint64_t target_offset,read_start;
static const uint8_t *target_digest;
static bool readable;
static unsigned readable_region;
static uintptr_t map_disk_dma,map_store_bytes;
static struct map_store_page control;
static struct map_store_page *page(void){return &control;}
static uint8_t *disk;
static unsigned pending_code;static uint64_t pending_at;static size_t pending_n;
static unsigned error,flushes;static bool pending,finished;
static bool submit(unsigned code,uint64_t at,size_t n){assert(!pending&&n<=CHUNK&&at+n<=MAP_PACK_BYTES);pending=true;pending_code=code;pending_at=at;pending_n=n;amount=n;return true;}
static void dma_read(void *d,const void *s,size_t n){memcpy(d,s,n);}
static void dma_write(void *d,const void *s,size_t n){memcpy(d,s,n);}
static uint64_t us(void){return 1;}
static void done(unsigned e){error=e;finished=true;}
static void read_next(void){size_t n=target_length-offset;if(n>CHUNK)n=CHUNK;assert(submit(0,target_offset+offset,(n+511)&~(size_t)511));}
static void microkit_dbg_puts(const char *s){(void)s;}
#include "../experiments/comaps-native/import_store.h"
static void perform(void){assert(pending);pending=false;uint8_t *payload=(void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET);
 if(pending_code==0)memcpy(payload,disk+pending_at,pending_n);else if(pending_code==1)memcpy(disk+pending_at,payload,pending_n);else{assert(pending_code==4);flushes++;}
 assert(local_complete());
}
static void pump(void){unsigned count=0;while(pending){perform();assert(++count<1000);}}
static void base(void){memset(local_regions,0,sizeof(local_regions));local_regions[0].identity[0]=1;strcpy(local_regions[0].name,"Gibraltar");local_regions[0].length=16;local_regions[0].version=1;local_regions[0].bank=2;local_regions[0].present=1;local_generation=1;local_bank=0;map_local_catalog_write(disk+MAP_IMPORT_CATALOG,local_regions,1);memcpy(disk+MAP_IMPORT_CATALOG+4096,disk+MAP_IMPORT_CATALOG,4096);}
static void prepare_update(unsigned version){memset(&import_pack,0,sizeof(import_pack));import_pack.region=0;import_pack.identity[0]=1;strcpy(import_pack.name,"Gibraltar");import_pack.length=200000;import_pack.version=version;memset((void*)map_store_bytes,version,import_pack.length);map_sha256((void*)map_store_bytes,import_pack.length,import_pack.digest);import_ready=true;import_probed=true;finished=false;error=999;}
static unsigned recover(struct map_local_region *out){uint64_t best=0,g;struct map_local_region temp[4];for(unsigned i=0;i<2;i++)if(map_local_catalog_read(disk+MAP_IMPORT_CATALOG+i*4096,temp,&g)&&g>best){best=g;memcpy(out,temp,sizeof(temp));}assert(best);return out[0].version;}
int main(void){
 disk=calloc(1,MAP_PACK_BYTES);map_disk_dma=(uintptr_t)calloc(1,MAP_IO_DMA_BYTES);map_store_bytes=(uintptr_t)calloc(1,MAP_FILE_LIMIT);assert(disk&&map_disk_dma&&map_store_bytes);base();
 uint8_t original[8192];memset(disk+MAP_PRIVATE_OFFSET,0x65,8192);memcpy(original,disk+MAP_PRIVATE_OFFSET,8192);
 struct map_local_region found[4];
 /* A crash after every real payload/readback/flush/catalogue I/O completion. */
 for(unsigned cut=0;cut<15;cut++){
  base();prepare_update(2);pending=false;phase=0;assert(local_request(MAP_IMPORT_COMMIT));unsigned n=0;
  while(pending&&n++<cut)perform();
  unsigned v=recover(found);assert(v==1||v==2);
  if(v==2){assert(found[0].bank==0&&flushes>0);uint8_t h[32];map_sha256(disk+MAP_IMPORT_EXTENT(0,0),found[0].length,h);assert(!memcmp(h,found[0].digest,32));}
  assert(!memcmp(original,disk+MAP_PRIVATE_OFFSET,8192));pending=false;
 }
 for(unsigned cut=0;cut<15;cut++){
  base();prepare_update(1);import_pack.region=3;import_pack.identity[0]=7;strcpy(import_pack.name,"Local region");pending=false;assert(local_request(MAP_IMPORT_COMMIT));unsigned n=0;
  while(pending&&n++<cut)perform();assert(recover(found)==1);assert(!found[3].present||found[3].version==1);assert(!memcmp(original,disk+MAP_PRIVATE_OFFSET,8192));pending=false;
 }
 /* Cancellation before catalogue publication retains the old version; after
  * publication starts it must complete, never report a false cancellation. */
 for(unsigned cut=0;cut<10;cut++){
  base();prepare_update(2);pending=false;control.cancel_sequence=0;control.sequence=42;assert(local_request(MAP_IMPORT_COMMIT));
  for(unsigned n=0;pending&&n<cut;n++)perform();
  if(pending){unsigned at=phase;control.cancel_sequence=42;pump();assert((at<=13&&error==MAP_STORE_CANCELLED&&recover(found)==1)||(at>=14&&error==0&&recover(found)==2));}
 }
 control.cancel_sequence=0;
 /* A damaged destination must never become the active version. */
 base();prepare_update(2);assert(local_request(MAP_IMPORT_COMMIT));while(phase==11)perform();assert(phase==12);disk[MAP_IMPORT_EXTENT(0,0)+42]^=1;pump();assert(error==MAP_STORE_CORRUPT&&recover(found)==1);
 base();prepare_update(2);assert(local_request(MAP_IMPORT_COMMIT));pump();assert(finished&&!error&&local_generation==2&&recover(found)==2);
 uint8_t good[4096];memcpy(good,disk+MAP_IMPORT_CATALOG+4096,4096);
 for(unsigned i=0;i<4096;i++){memcpy(disk+MAP_IMPORT_CATALOG+4096,good,4096);disk[MAP_IMPORT_CATALOG+4096+i]^=1;assert(recover(found)==1);}
 memcpy(disk+MAP_IMPORT_CATALOG+4096,good,4096);
 prepare_update(3);assert(local_request(MAP_IMPORT_COMMIT));pump();assert(!error&&recover(found)==3&&local_regions[0].bank==1);
 prepare_update(2);assert(local_request(MAP_IMPORT_COMMIT)&&finished&&error==MAP_STORE_INVALID&&!pending);assert(recover(found)==3);
 prepare_update(4);import_pack.identity[0]=9;assert(local_request(MAP_IMPORT_COMMIT)&&error==MAP_STORE_INVALID);
 /* Header validation and identity/version matching use the actual parser. */
 uint8_t h[4096]={0};memcpy(h,"NXMRPK01",8);uint32_t fmt=1,r=3;uint64_t ver=1,len=64;memcpy(h+8,&fmt,4);memcpy(h+12,&r,4);memcpy(h+16,&ver,8);memcpy(h+24,&len,8);h[32]=9;strcpy((char*)h+48,"Local region");map_sha256(h,256,h+256);struct map_local_pack p;assert(map_local_pack_read(h,&p));
 h[144]=1;map_sha256(h,256,h+256);assert(!map_local_pack_read(h,&p));h[144]=0;r=2;memcpy(h+12,&r,4);map_sha256(h,256,h+256);assert(!map_local_pack_read(h,&p));
 assert(!memcmp(original,disk+MAP_PRIVATE_OFFSET,8192));
 local_generation=0;pending=false;finished=false;assert(local_request(MAP_IMPORT_PROBE)&&finished&&error==MAP_STORE_CORRUPT&&!pending);
 free(disk);free((void*)map_disk_dma);free((void*)map_store_bytes);
 puts("PASS guest import I/O crash cuts, torn catalogue sectors, alternating updates, downgrade/identity rejection and untouched private banks");return 0;
}
