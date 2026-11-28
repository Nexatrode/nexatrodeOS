/* SPDX-License-Identifier: Apache-2.0 */
#include "../files-picker/protocol.h"
uintptr_t files_state,files_chunk;
static uint64_t files_token[2],files_offset;
static uint8_t files_header[4096];
static bool files_source;
static void files_release(void){
 if(!files_source)return;microkit_mr_set(0,files_token[0]);microkit_mr_set(1,files_token[1]);microkit_ppcall(3,microkit_msginfo_new(FILES_RELEASE,2));
#ifdef MAP_FILES_VERIFY
 microkit_mr_set(0,files_token[0]);microkit_mr_set(1,files_token[1]);microkit_mr_set(2,0);microkit_mr_set(3,1);
 if(!microkit_msginfo_get_label(microkit_ppcall(3,microkit_msginfo_new(FILES_READ,4))))__builtin_trap();
 microkit_dbg_puts("FILES|REVOKED|stale read denied\n");
#endif
 files_source=false;memset(files_token,0,sizeof(files_token));
}
static bool files_read(uint64_t off,void*out,unsigned n){
 microkit_mr_set(0,files_token[0]);microkit_mr_set(1,files_token[1]);microkit_mr_set(2,off);microkit_mr_set(3,n);microkit_msginfo r=microkit_ppcall(3,microkit_msginfo_new(FILES_READ,4));struct files_chunk*c=(void*)files_chunk;
 if(microkit_msginfo_get_label(r)||c->length!=n)return false;memcpy(out,c->bytes,n);return true;
}
static void files_step(void){
 if(!busy||!files_source||(phase!=16&&phase!=17))return;
 if(import_cancelled()){import_ready=import_probed=false;done(MAP_STORE_CANCELLED);return;}
 if(phase==16){unsigned n=4096-files_offset;if(n>FILES_CHUNK)n=FILES_CHUNK;
  if(!files_read(files_offset,files_header+files_offset,n)){done(MAP_STORE_INVALID);return;}files_offset+=n;
  if(files_offset<4096)return;
  memset(&import_pack,0,sizeof(import_pack));import_ready=import_probed=false;
  if(!map_local_pack_read(files_header,&import_pack)||!map_local_update_allowed(&import_pack,&local_regions[import_pack.region])){done(MAP_STORE_INVALID);return;}
  struct files_state*s=(void*)files_state;if(s->length!=4096+import_pack.length){done(MAP_STORE_INVALID);return;}
  for(unsigned i=0;i<4;i++)if(i!=import_pack.region&&local_regions[i].present&&!memcmp(local_regions[i].identity,import_pack.identity,16)){done(MAP_STORE_INVALID);return;}
  import_probed=true;region=import_pack.region;done(MAP_STORE_OK);return;
 }
 unsigned n=import_pack.length-offset;if(n>FILES_CHUNK)n=FILES_CHUNK;
 if(!files_read(4096+offset,(void*)(map_store_bytes+offset),n)){done(MAP_STORE_INVALID);return;}offset+=n;page()->progress=offset;
 if(offset<import_pack.length)return;
 uint8_t hash[32];map_sha256((void*)map_store_bytes,offset,hash);if(memcmp(hash,import_pack.digest,32)){done(MAP_STORE_CORRUPT);return;}
 target_length=offset;import_ready=true;files_release();done(MAP_STORE_OK);
}
static bool files_begin(void){
 struct files_state*s=(void*)files_state;unsigned r=__atomic_load_n(&s->revision,__ATOMIC_ACQUIRE);uint64_t a=s->token[0],b=s->token[1],length=s->length,status=s->status;atomic_thread_fence(memory_order_acquire);
 if(!r||(r&1)||r!=__atomic_load_n(&s->revision,__ATOMIC_ACQUIRE)||status!=1||!length||length>FILES_LIMIT||!(a||b))return false;
 files_token[0]=a;files_token[1]=b;files_source=true;
#ifdef MAP_FILES_VERIFY
 // Exercise the real provider with the selected-service endpoint: no directory
 // API, forged grant, out-of-range or oversized read may succeed.
 for(unsigned check=0;check<4;check++){
  microkit_mr_set(0,check==0?a^1:a);microkit_mr_set(1,b);microkit_mr_set(2,check==1?length:0);microkit_mr_set(3,check==2?FILES_CHUNK+1:1);
  microkit_msginfo bad=microkit_ppcall(3,microkit_msginfo_new(check==3?0x44a:FILES_READ,4));
  if(!microkit_msginfo_get_label(bad)){files_release();return false;}
 }
 microkit_dbg_puts("FILES|DENIALS|forged,bounds,oversize,directory\n");
#endif
 files_offset=0;phase=16;page()->import_phase=16;files_step();return true;
}
