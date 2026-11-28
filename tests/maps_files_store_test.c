/* Actual selected-file reader and transaction state machines, mocked IPC only. */
#include <stdatomic.h>
#define MAP_FILES 1
#define main baseline_main
#include "maps_import_test.c"
#undef main
#include "../experiments/files-picker/protocol.h"
typedef struct {unsigned label,count;} microkit_msginfo;
static uint64_t mr[4];
static uintptr_t source;static size_t source_size;static bool busy=true,revoked;
static void microkit_mr_set(unsigned i,uint64_t v){mr[i]=v;}
static microkit_msginfo microkit_msginfo_new(unsigned l,unsigned n){return (microkit_msginfo){l,n};}
static unsigned microkit_msginfo_get_label(microkit_msginfo m){return m.label;}
static microkit_msginfo microkit_ppcall(unsigned ch,microkit_msginfo m);
#include "../experiments/comaps-native/files_store.h"
static microkit_msginfo microkit_ppcall(unsigned ch,microkit_msginfo m){
 assert(ch==3);struct files_chunk*c=(void*)files_chunk;memset(c,0,4096);
 if(m.label==FILES_RELEASE){revoked=true;return microkit_msginfo_new(0,0);}
 if(revoked||mr[0]!=44||mr[1]!=55||mr[2]>source_size||mr[3]>source_size-mr[2]||mr[3]>FILES_CHUNK)return microkit_msginfo_new(1,0);
 assert(m.label==FILES_READ);c->length=mr[3];memcpy(c->bytes,(void*)(source+mr[2]),mr[3]);return microkit_msginfo_new(0,0);
}
static void selected(void){
 struct files_state*s=(void*)files_state;memset(s,0,4096);s->revision=2;s->status=1;s->token[0]=44;s->token[1]=55;s->length=source_size;revoked=false;finished=false;control.cancel_sequence=0;control.sequence++;
}
static void read_pump(void){for(unsigned n=0;!finished;n++){assert(n<1000);files_step();}}
int main(void){
 struct files_grant g={{44,55},123,10000};
 assert(files_permits(&g,44,55,0,3072));assert(files_permits(&g,44,55,9999,1));
 assert(!files_permits(&g,44,56,0,1));assert(!files_permits(&g,0,0,0,1));
 assert(!files_permits(&g,44,55,UINT64_MAX,1));assert(!files_permits(&g,44,55,10000,1));
 assert(!files_permits(&g,44,55,0,3073));assert(!files_permits(&g,44,55,1,0));
 files_revoke(&g);assert(!files_permits(&g,44,55,0,1));for(unsigned i=0;i<sizeof(g);i++)assert(!((uint8_t*)&g)[i]);
 disk=calloc(1,MAP_PACK_BYTES);map_disk_dma=(uintptr_t)calloc(1,MAP_IO_DMA_BYTES);map_store_bytes=(uintptr_t)calloc(1,MAP_FILE_LIMIT);files_state=(uintptr_t)calloc(1,4096);files_chunk=(uintptr_t)calloc(1,4096);base();prepare_update(2);
 source_size=4096+import_pack.length;source=(uintptr_t)calloc(1,source_size);uint8_t*h=(void*)source;memcpy(h,"NXMRPK01",8);uint32_t fmt=1;memcpy(h+8,&fmt,4);memcpy(h+12,&import_pack.region,4);memcpy(h+16,&import_pack.version,8);memcpy(h+24,&import_pack.length,8);memcpy(h+32,import_pack.identity,16);memcpy(h+48,import_pack.name,64);memcpy(h+112,import_pack.digest,32);map_sha256(h,256,h+256);memcpy(h+4096,(void*)map_store_bytes,import_pack.length);
 selected();assert(local_request(MAP_IMPORT_PROBE));read_pump();assert(!error&&import_probed&&!revoked);
 finished=false;assert(local_request(MAP_IMPORT_READ));read_pump();assert(!error&&import_ready&&revoked);
 finished=false;assert(local_request(MAP_IMPORT_COMMIT));pump();assert(!error&&local_regions[0].version==2);
 // Cancel a live background read; no installation or grant survives it.
 base();selected();assert(local_request(MAP_IMPORT_PROBE));read_pump();finished=false;assert(local_request(MAP_IMPORT_READ));assert(!finished&&offset==FILES_CHUNK);control.cancel_sequence=control.sequence;read_pump();assert(error==MAP_STORE_CANCELLED);files_release();assert(revoked&&local_regions[0].version==1);
 // Corruption and a grant revoked halfway through cannot become installable.
 selected();assert(local_request(MAP_IMPORT_PROBE));read_pump();h[4097]^=1;finished=false;assert(local_request(MAP_IMPORT_READ));read_pump();assert(error==MAP_STORE_CORRUPT&&!import_ready);files_release();h[4097]^=1;
 selected();assert(local_request(MAP_IMPORT_PROBE));read_pump();finished=false;assert(local_request(MAP_IMPORT_READ));revoked=true;read_pump();assert(error==MAP_STORE_INVALID&&!import_ready);
 puts("PASS selected-file bounds, stale grants, background reads, cancellation, corrupt input and durable install");
 free(disk);free((void*)map_disk_dma);free((void*)map_store_bytes);free((void*)files_state);free((void*)files_chunk);free((void*)source);
}
