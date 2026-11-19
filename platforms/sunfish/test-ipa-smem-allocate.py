#!/usr/bin/env python3
"""Exercise the actual one-shot C allocator, including partial publication."""
from pathlib import Path
import argparse
import importlib.util
import json
import shutil
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("census_tests", HERE / "test-ipa-smem-census.py")
census = importlib.util.module_from_spec(spec)
spec.loader.exec_module(census)
TEST = r'''
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#define SMEM_PROBE_HOST_TEST 1
static uint32_t smem_alloc_test_lock_read(void);
static void smem_alloc_test_lock_write(uint32_t value);
static void smem_alloc_test_write(uint32_t offset,uint32_t value);
static void check_read(uint32_t offset,uint32_t size);
#define SMEM_PROBE_READ_CHECK check_read
#include ALLOC_SOURCE
static uint8_t *image,*before;
static uint32_t owner, lock_reads, lock_claims, lock_releases, writes;
static uint32_t fail_write, lose_owner, change_pre, change_post, corrupt_post, foreign;
static uint32_t touched[4];
static uint32_t read_word(uint32_t at) {uint32_t v;memcpy(&v,image+at,4);return v;}
static void u16(uint32_t at,uint16_t value) {memcpy(image+at,&value,2);}
static void u32(uint32_t at,uint32_t value) {memcpy(image+at,&value,4);}
static uint32_t info_at(void) {return SMEM_BYTES-4096+32+read_word(SMEM_BYTES-4096+8)*48;}
static void check_read(uint32_t at,uint32_t size) {
    assert(at<=SMEM_BYTES && size<=SMEM_BYTES-at);
    /* Assert every parser/readback scalar avoids the real candidate payload,
     * including the portion sharing a host VM page with its header. */
    assert(!(at<ALLOC_HEADER_OFFSET+16+ALLOC_ITEM_BYTES && ALLOC_HEADER_OFFSET+16<at+size));
}
static uint32_t smem_alloc_test_lock_read(void) {
    ++lock_reads;
    if (lose_owner && lock_reads==lose_owner) owner=2;
    if (change_pre && lock_reads==2) u32(ALLOC_PART_OFFSET+12,0x30);
    if (change_post && lock_reads==11) u16(ALLOC_HEADER_OFFSET+2,496);
    return owner;
}
static void smem_alloc_test_lock_write(uint32_t value) {
    /* Keep the actual register accessor compiled and exercised on the host. */
    assert(lock_register()!=NULL);
    if (value) {++lock_claims;if(!foreign && !owner)owner=value;}
    else {++lock_releases;assert(owner==1);owner=0;}
}
static void smem_alloc_test_write(uint32_t at,uint32_t value) {
    assert(owner==1 && writes<4);
    const uint32_t offsets[]={ALLOC_HEADER_OFFSET,ALLOC_HEADER_OFFSET+4,ALLOC_HEADER_OFFSET+8,ALLOC_PART_OFFSET+12};
    assert(at==offsets[writes]);touched[writes]=at;++writes;
    assert(alloc_submitted==((1u<<writes)-1));
    assert(read_word(ALLOC_PART_OFFSET+12)==ALLOC_FREE_BEFORE);
    if(writes==4) {
        assert(read_word(ALLOC_HEADER_OFFSET)==((497u<<16)|0xa5a5));
        assert(read_word(ALLOC_HEADER_OFFSET+4)==8192);
        assert(read_word(ALLOC_HEADER_OFFSET+8)==0);
    }
    u32(at,fail_write==writes?value^1u:value);
    if(corrupt_post && writes==4)u16(ALLOC_HEADER_OFFSET,0);
}
static void fixture(void) {
    memset(image,0xcc,SMEM_BYTES);smem_vaddr=(uintptr_t)image;
    smem_metadata_vaddr=smem_vaddr+ALLOC_PART_OFFSET;tcsr_lock_vaddr=(uintptr_t)&owner;
    owner=0;lock_reads=lock_claims=lock_releases=writes=0;
    fail_write=lose_owner=change_pre=change_post=corrupt_post=foreign=0;
    alloc_consumed=alloc_pre_scans=alloc_post_scans=alloc_submitted=alloc_completed=alloc_published=alloc_item_count=0;
    census_attempts=census_scans=census_release_sent=0;
    u32(192,1);u32(204,0);u32(92,12u<<16);
    uint32_t table=SMEM_BYTES-4096;memset(image+table,0,4096);
    memcpy(image+table,"$TOC",4);u32(table+4,1);
#ifdef SMEM_IPA_ALLOC_V2
    u32(table+8,11);u32(table+32,table);u32(table+36,4096);
    u16(table+32+12,0xffff);u16(table+32+14,0xffff);
#else
    u32(table+8,8);
#endif
    uint32_t entry=table+32+7*48;
    u32(entry,ALLOC_PART_OFFSET);u32(entry+4,ALLOC_PART_BYTES);
    u16(entry+12,0);u16(entry+14,1);u32(entry+16,32);
    uint32_t p=ALLOC_PART_OFFSET;
    memcpy(image+p,"$PRT",4);u16(p+4,0);u16(p+6,1);u32(p+8,ALLOC_PART_BYTES);
    u32(p+12,32);u32(p+16,ALLOC_PART_BYTES);
    uint32_t info=info_at();memcpy(image+info,"SIII",4);u16(info+16,621);
    memcpy(before,image,SMEM_BYTES);
}
static int run(void) {
    struct item_result item={0};struct route_result route={0};
    return allocate_once(&item,&route);
}
static void only_metadata_changed(void) {
    for(uint32_t i=0;i<SMEM_BYTES;++i) {
        int allowed=0;
        for(uint32_t j=0;j<writes;++j)if(i>=touched[j] && i<touched[j]+4)allowed=1;
        if(!allowed)assert(image[i]==before[i]);
    }
}
static void repeat_rejected(void) {
    uint32_t n=writes,reads=lock_reads,claims=lock_claims,releases=lock_releases;
    assert(run()==-40);assert(writes==n && lock_reads==reads && lock_claims==claims && lock_releases==releases);
}
int main(void) {
    image=mmap(NULL,SMEM_BYTES,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON,-1,0);assert(image!=MAP_FAILED);
    before=malloc(SMEM_BYTES);assert(before);
    fixture();assert(run()==0);assert(writes==4 && alloc_submitted==15 && alloc_completed==15 && alloc_published==1);
    assert(alloc_pre_scans==2 && alloc_post_scans==2 && census_scans==4 && census_release_sent==1);
    assert(lock_claims==1 && lock_releases==1 && owner==0);
    assert(read_word(ALLOC_PART_OFFSET+12)==0x2030);only_metadata_changed();repeat_rejected();
    struct item_result item={0};struct route_result route={0};
    assert(find_items(&item,&route)==0 && item.found && item.offset==0x11d030 && item.size==8192 && !item.cached);
    /* Existing allocation is not adopted as a new ownership receipt. */
    alloc_consumed=0;writes=0;assert(run()==-41 && !writes);repeat_rejected();
    /* Every uncertain write consumes the attempt; no rollback or next write. */
    for(uint32_t n=1;n<=4;++n) {
        fixture();fail_write=n;assert(run()==-45);assert(writes==n);
        assert(alloc_submitted==((1u<<n)-1) && alloc_completed==((1u<<(n-1))-1));
        assert(!alloc_published && lock_releases==1);only_metadata_changed();repeat_rejected();
    }
    /* Lose ownership at every check after the successful claim. Never release
     * or force a foreign lock even after a partially published allocation. */
    for(uint32_t n=2;n<=12;++n) {
        fixture();lose_owner=n;assert(run()==-31 && owner==2 && !lock_releases);
        only_metadata_changed();repeat_rejected();
    }
    fixture();foreign=1;owner=2;assert(run()==-7 && !writes && !lock_releases && census_attempts==HWLOCK_TRIES);repeat_rejected();
    fixture();change_pre=1;assert(run()==-30 && !writes && lock_releases==1);repeat_rejected();
    fixture();change_post=1;assert(run()==-30 && writes==4 && alloc_post_scans==2 && lock_releases==1);repeat_rejected();
    fixture();corrupt_post=1;assert(run()==-46 && writes==4 && alloc_published && lock_releases==1);repeat_rejected();
    fixture();smem_metadata_vaddr+=4;assert(run()==-41 && !writes);repeat_rejected();
    fixture();
    uint32_t cached=ALLOC_PART_OFFSET+ALLOC_PART_BYTES-32;
    u16(cached,0xa5a5);u16(cached+2,497);u32(cached+4,8192);u16(cached+8,0);
    u32(ALLOC_PART_OFFSET+16,ALLOC_PART_BYTES-32-8192-32);
    assert(run()==-41 && !writes);repeat_rejected();
    fixture();u32(ALLOC_PART_OFFSET+16,ALLOC_PART_BYTES-32);assert(run()!=0 && !writes);repeat_rejected();
    fixture();u32(SMEM_BYTES-4096+32+7*48+16,64);assert(run()==-41 && !writes);repeat_rejected();
    fixture();u32(SMEM_BYTES-4096+32+48,ALLOC_PART_OFFSET+4096);u32(SMEM_BYTES-4096+36+48,4096);
    assert(run()==-43 && !writes);repeat_rejected();
    fixture();u32(SMEM_BYTES-4096+32+48,0xfffffff0);u32(SMEM_BYTES-4096+36+48,4096);
    assert(run()==-43 && !writes);repeat_rejected();
    fixture();u16(info_at()+16,497);assert(run()==-42 && !writes);repeat_rejected();
    fixture();u32(info_at(),0);memcpy(before,image,SMEM_BYTES);
    assert(run()==0 && alloc_item_count==512);only_metadata_changed();repeat_rejected();
    fixture();u32(192,0);assert(run()==-1 && !writes && lock_releases==1);repeat_rejected();
    /* Reject an out-of-order, wrong-value or wrong-offset operation without a store. */
    fixture();owner=1;
    assert(alloc_write(ALLOC_HEADER_OFFSET+4,8192,2)==-44);
    assert(alloc_write(ALLOC_HEADER_OFFSET,0,1)==-44);
    assert(alloc_write(ALLOC_HEADER_OFFSET+12,(497u<<16)|0xa5a5,1)==-44 && !writes);
    unsigned extra=0;
#ifdef SMEM_IPA_ALLOC_V2
    /* Reproduce the complete observed guard inputs: a TOC self-descriptor,
     * one half-empty slot and no data-partition overlap. */
    fixture();u32(SMEM_BYTES-4096+32+48,0);u32(SMEM_BYTES-4096+36+48,4096);
    memcpy(before,image,SMEM_BYTES);assert(run()==0 && writes==4);only_metadata_changed();repeat_rejected();++extra;
    fixture();u32(SMEM_BYTES-4096+32+48,4096);u32(SMEM_BYTES-4096+36+48,0);
    memcpy(before,image,SMEM_BYTES);assert(run()==0 && writes==4);only_metadata_changed();repeat_rejected();++extra;
    const uint32_t toc_mutations[]={32,36,44,46};
    for(unsigned i=0;i<4;++i) {
        fixture();uint32_t at=SMEM_BYTES-4096+toc_mutations[i];
        if(i<2)u32(at,read_word(at)^4096);else u16(at,0xfffe);
        assert(run()==-47 && !writes);repeat_rejected();++extra;
    }
    fixture();u32(SMEM_BYTES-4096+8,10);assert(run()==-47 && !writes);repeat_rejected();++extra;
    /* The exception applies only at row zero; another TOC-shaped span remains
     * out of range even with the same reserved host values. */
    fixture();memcpy(image+SMEM_BYTES-4096+32+48,image+SMEM_BYTES-4096+32,48);
    assert(run()==-43 && !writes);repeat_rejected();++extra;
    fixture();u32(SMEM_BYTES-4096+32+48,0x20000);u32(SMEM_BYTES-4096+36+48,4096);
    memcpy(before,image,SMEM_BYTES);assert(run()==0 && writes==4);only_metadata_changed();repeat_rejected();++extra;
#else
    /* Preserve the original failure when replaying the measured TOC span. */
    fixture();u32(SMEM_BYTES-4096+32,SMEM_BYTES-4096);u32(SMEM_BYTES-4096+36,4096);
    u16(SMEM_BYTES-4096+44,0xffff);u16(SMEM_BYTES-4096+46,0xffff);
    assert(run()==-43 && !writes);repeat_rejected();++extra;
#endif
    assert(munmap(image,SMEM_BYTES)==0);free(before);
    printf("%u C allocation fixture groups passed, including all 4 uncertain writes and 11 ownership-loss boundaries; only the 4 planned metadata words changed and no item payload scalar was read.\n",18+extra);
    return 0;
}
'''

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--v2", action="store_true")
    args = parser.parse_args()
    clang = shutil.which("clang")
    if not clang:
        raise SystemExit("clang is required")
    with tempfile.TemporaryDirectory(prefix="ipa-smem-allocate-") as name:
        temporary = Path(name)
        (temporary / "microkit.h").write_text(census.STUB)
        source = "ipa-smem-allocate-v2.c" if args.v2 else "ipa-smem-allocate.c"
        (temporary / "test.c").write_text(TEST.replace("ALLOC_SOURCE", json.dumps(str(HERE / source))))
        subprocess.run([clang, "-O2", "-Wall", "-Wextra", "-Werror", "-I", str(temporary), str(temporary / "test.c"), "-o", str(temporary / "test")], check=True)
        subprocess.run([str(temporary / "test")], check=True)
