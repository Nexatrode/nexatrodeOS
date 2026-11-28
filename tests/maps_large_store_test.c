/* Real paged installer with mocked disk completions and selected-source IPC. */
#define MAP_IMPORT 1
#define MAP_FILES 1
#define MAP_LARGE_IMPORT 1
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include <stdatomic.h>
#include "../experiments/comaps-native/import_format.h"
#define CHUNK MAP_IO_CHUNK
static unsigned phase,region,amount;
static size_t offset,target_length;
static uint64_t target_offset,read_start;
static const uint8_t *target_digest;
static bool readable,busy=true;
static unsigned readable_region;
static uintptr_t map_disk_dma,map_store_bytes;
static struct map_store_page control;
static struct map_store_page *page(void){return &control;}
static uint8_t *disk,*source;static size_t source_size;
static unsigned pending_code;static uint64_t pending_at;static size_t pending_n;
static unsigned error;static bool pending,finished,revoked;
static bool submit(unsigned code,uint64_t at,size_t n){assert(!pending&&n<=CHUNK&&at+n<=MAP_PACK_BYTES);pending=true;pending_code=code;pending_at=at;pending_n=n;amount=n;return true;}
static void dma_read(void*d,const void*s,size_t n){memcpy(d,s,n);}
static void dma_write(void*d,const void*s,size_t n){memcpy(d,s,n);}
static uint64_t us(void){return 1;}
static void files_release(void);
static void done(unsigned e){error=e;finished=true;if(e)files_release();}
static void read_next(void){size_t n=target_length-offset;if(n>CHUNK)n=CHUNK;assert(submit(0,target_offset+offset,(n+511)&~(size_t)511));}
static void microkit_dbg_puts(const char*s){(void)s;}
#include "../experiments/comaps-native/import_store.h"
typedef struct{unsigned label,count;} microkit_msginfo;
static uint64_t mr[4];static void microkit_mr_set(unsigned i,uint64_t v){mr[i]=v;}
static microkit_msginfo microkit_msginfo_new(unsigned l,unsigned n){return(microkit_msginfo){l,n};}
static unsigned microkit_msginfo_get_label(microkit_msginfo m){return m.label;}
static microkit_msginfo microkit_ppcall(unsigned,microkit_msginfo);
#include "../experiments/comaps-native/large_store.h"
#include "../experiments/comaps-native/files_store.h"
static microkit_msginfo microkit_ppcall(unsigned ch,microkit_msginfo m){assert(ch==3);struct files_chunk*c=(void*)files_chunk;memset(c,0,4096);
 if(m.label==FILES_RELEASE){revoked=true;return microkit_msginfo_new(0,0);}
 if(revoked||mr[0]!=44||mr[1]!=55||mr[2]>source_size||mr[3]>source_size-mr[2]||mr[3]>FILES_CHUNK)return microkit_msginfo_new(1,0);
 assert(m.label==FILES_READ);c->length=mr[3];memcpy(c->bytes,source+mr[2],mr[3]);return microkit_msginfo_new(0,0);
}
static void step(void){if(!pending){files_step();return;}pending=false;uint8_t*p=(void*)(map_disk_dma+MAP_IO_PAYLOAD_OFFSET);
 if(pending_code==0)memcpy(p,disk+pending_at,pending_n);else if(pending_code==1)memcpy(disk+pending_at,p,pending_n);else assert(pending_code==4);
 assert(bundle_complete()||local_complete());
}
static void pump(void){for(unsigned n=0;!finished;n++){assert(n<100000);step();}}
static void base(void){memset(local_regions,0,sizeof(local_regions));local_generation=1;local_bank=0;map_local_catalog_write(disk+MAP_IMPORT_CATALOG,local_regions,1);memcpy(disk+MAP_IMPORT_CATALOG+4096,disk+MAP_IMPORT_CATALOG,4096);}
static unsigned installed(void){uint64_t best=0,g;struct map_local_region r[4];unsigned v=0;for(unsigned i=0;i<2;i++)if(map_local_catalog_read(disk+MAP_IMPORT_CATALOG+i*4096,r,&g)&&g>best){best=g;v=r[3].present;}assert(best);return v;}
static void select_source(void){struct files_state*s=(void*)files_state;memset(s,0,4096);s->revision=2;s->status=1;s->token[0]=44;s->token[1]=55;s->length=source_size;revoked=false;finished=false;pending=false;control.cancel_sequence=0;control.sequence++;control.command=MAP_IMPORT_PROBE;assert(local_request(MAP_IMPORT_PROBE));pump();assert(!error&&import_probed&&import_pack.kind==2);}
static void begin(void){finished=false;control.sequence++;control.command=MAP_IMPORT_READ;assert(local_request(MAP_IMPORT_READ));}
static void rehash(void){uint8_t*d=source+4096;map_sha256(d,MAP_DIRECTORY_BYTES,source+112);map_sha256(source,256,source+256);}
int main(int argc,char**argv){assert(argc==2);FILE*f=fopen(argv[1],"rb");assert(f);fseek(f,0,SEEK_END);source_size=ftell(f);rewind(f);source=malloc(source_size);assert(fread(source,1,source_size,f)==source_size);fclose(f);assert(source_size>MAP_FILE_LIMIT);
 disk=calloc(1,MAP_PACK_BYTES);map_disk_dma=(uintptr_t)calloc(1,MAP_IO_DMA_BYTES);map_store_bytes=(uintptr_t)calloc(1,MAP_FILE_LIMIT);files_state=(uintptr_t)calloc(1,4096);files_chunk=(uintptr_t)calloc(1,4096);assert(disk&&map_disk_dma&&map_store_bytes&&files_state&&files_chunk);
 memset(disk+MAP_PRIVATE_OFFSET,0x65,8192);uint8_t private[8192];memcpy(private,disk+MAP_PRIVATE_OFFSET,8192);
 // Canonical directory rejects offsets, capacities, nonfinite coordinates,
 // truncated extents, reserved data and version inconsistencies.
 uint8_t gooddir[MAP_DIRECTORY_BYTES],mutdir[MAP_DIRECTORY_BYTES];memcpy(gooddir,source+4096,sizeof(gooddir));struct map_directory decoded;assert(map_directory_read(gooddir,&decoded));
 const unsigned bad_offsets[]={0,8,12,16,96,256,256+sizeof(decoded.segments)};
 for(unsigned i=0;i<sizeof(bad_offsets)/sizeof(*bad_offsets);i++){memcpy(mutdir,gooddir,sizeof(mutdir));mutdir[bad_offsets[i]]^=1;assert(!map_directory_read(mutdir,&decoded));}
 memcpy(mutdir,gooddir,sizeof(mutdir));double nan=NAN;memcpy(mutdir+24,&nan,8);assert(!map_directory_read(mutdir,&decoded));
 memcpy(mutdir,gooddir,sizeof(mutdir));uint64_t over=MAP_FILE_LIMIT+1;memcpy(mutdir+264,&over,8);assert(!map_directory_read(mutdir,&decoded));
 // Cancel real source, write and readback phases. No partially installed entry.
 for(unsigned cancel_phase=30;cancel_phase<=32;cancel_phase++){
  base();select_source();begin();for(unsigned n=0;phase!=cancel_phase;n++){assert(n<100000&&!finished);step();}
  control.cancel_sequence=control.sequence;pump();assert(error==MAP_STORE_CANCELLED&&revoked&&!installed());assert(!memcmp(private,disk+MAP_PRIVATE_OFFSET,8192));
 }
 // Structural damage with recomputed checksums must still fail before publication.
 base();select_source();struct map_segment seg=bundle_directory.segments[1];uint8_t original[4];memcpy(original,source+4096+seg.offset+4,4);memset(source+4096+seg.offset+4,255,4);map_sha256(source+4096+seg.offset,seg.length,source+4096+256+80+16);rehash();select_source();begin();pump();assert(error==MAP_STORE_CORRUPT&&!installed());
 memcpy(source+4096+seg.offset+4,original,4);map_sha256(source+4096+seg.offset,seg.length,source+4096+256+80+16);rehash();
 base();select_source();begin();pump();assert(!error&&import_ready&&revoked&&!installed());assert(page()->peak_buffered<=MAP_FILE_LIMIT&&page()->peak_buffered<source_size);
 // Payload is staged and verified, but every commit cut is old or complete new.
 for(unsigned cut=0;cut<=3;cut++){
  map_local_catalog_write(disk+MAP_IMPORT_CATALOG,local_regions,1);memcpy(disk+MAP_IMPORT_CATALOG+4096,disk+MAP_IMPORT_CATALOG,4096);local_generation=1;local_bank=0;import_ready=true;finished=false;pending=false;control.cancel_sequence=0;control.command=MAP_IMPORT_COMMIT;assert(local_request(MAP_IMPORT_COMMIT));
  for(unsigned n=0;pending&&n<cut;n++)step();unsigned v=installed();assert(v==0||v==2);pending=false;
 }
 base();select_source();begin();pump();finished=false;control.command=MAP_IMPORT_COMMIT;assert(local_request(MAP_IMPORT_COMMIT));pump();assert(!error&&installed()==2&&local_regions[3].length>MAP_FILE_LIMIT);assert(!memcmp(private,disk+MAP_PRIVATE_OFFSET,8192));
 printf("PASS paged source=%zu bytes peak_workspace=%llu bytes, all pages/search, cancellation, structural corruption, commit crash cuts and untouched private banks\n",source_size,(unsigned long long)page()->peak_buffered);
 free(source);free(disk);free((void*)map_disk_dma);free((void*)map_store_bytes);free((void*)files_state);free((void*)files_chunk);return 0;
}
