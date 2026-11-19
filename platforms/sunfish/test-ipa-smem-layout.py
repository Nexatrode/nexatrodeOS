#!/usr/bin/env python3
"""Test actual read-only C table capture and stopped-guard classification."""
from pathlib import Path
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
static uint32_t smem_layout_test_lock_read(void);
static void smem_layout_test_lock_write(uint32_t value);
static void check_read(uint32_t offset,uint32_t size);
#define SMEM_PROBE_READ_CHECK check_read
#include LAYOUT_SOURCE
static uint8_t *image,*before;
static uint32_t owner,reads,claims,releases,foreign,lose_on_release;
static uint32_t smem_layout_test_lock_read(void) {++reads;if(lose_on_release && reads==2)owner=2;return owner;}
static void smem_layout_test_lock_write(uint32_t value) {
    assert(lock_register()!=NULL);
    if(value){++claims;if(!foreign && !owner)owner=value;}
    else {assert(owner==1);++releases;owner=0;}
}
static void check_read(uint32_t at,uint32_t size) {
    assert(at<=SMEM_BYTES && size<=SMEM_BYTES-at);
    assert(!(at<0x11f030 && 0x11d030<at+size));
}
static void u16(uint32_t at,uint16_t value) {memcpy(image+at,&value,2);}
static void u32(uint32_t at,uint32_t value) {memcpy(image+at,&value,4);}
static uint32_t row_at(uint32_t i) {return SMEM_BYTES-4096+32+i*48;}
static void row(uint32_t i,uint32_t offset,uint32_t size,uint16_t a,uint16_t b) {
    uint32_t e=row_at(i);u32(e,offset);u32(e+4,size);u16(e+12,a);u16(e+14,b);u32(e+16,32);
}
static void fixture(void) {
    assert(mprotect(image,SMEM_BYTES,PROT_READ|PROT_WRITE)==0);
    memset(image,0xcc,SMEM_BYTES);smem_vaddr=(uintptr_t)image;tcsr_lock_vaddr=(uintptr_t)&owner;
    owner=reads=claims=releases=foreign=lose_on_release=0;
    census_attempts=census_scans=census_release_sent=0;
    memset(&layout_first,0,sizeof(layout_first));memset(&layout_second,0,sizeof(layout_second));
    u32(192,1);u32(204,0);u32(92,12u<<16);
    uint32_t t=SMEM_BYTES-4096;memset(image+t,0,4096);memcpy(image+t,"$TOC",4);u32(t+4,1);u32(t+8,8);
    row(7,0x11d000,0x7c000,0,1);
    uint32_t p=0x11d000;memcpy(image+p,"$PRT",4);u16(p+4,0);u16(p+6,1);
    u32(p+8,0x7c000);u32(p+12,32);u32(p+16,0x7c000);
    uint32_t info=t+32+8*48;memcpy(image+info,"SIII",4);u16(info+16,621);
}
static struct layout_guard inspect(void) {
    assert(capture_layout(&layout_first)==0);return classify_layout(&layout_first);
}
int main(void) {
    image=mmap(NULL,SMEM_BYTES,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON,-1,0);assert(image!=MAP_FAILED);
    before=malloc(SMEM_BYTES);assert(before);unsigned groups=0;struct layout_guard g;
    fixture();g=inspect();assert(!g.mask && g.first_index==0xffffffff && layout_first.count==8 && layout_first.item_limit==621);++groups;
    /* Host VM makes the entire SMEM image read-only, including the page which
     * would have been writable in the stopped allocation test. */
    fixture();memcpy(before,image,SMEM_BYTES);assert(mprotect(image,SMEM_BYTES,PROT_READ)==0);
    assert(read_layout_once()==0 && census_scans==2 && census_release_sent==1 && claims==1 && releases==1);
    assert(memcmp(image,before,SMEM_BYTES)==0);++groups;
    fixture();row(0,0,4096,2,3);g=inspect();assert(g.mask==1 && g.first_index==0 && g.first_cause==1 && g.partial_empty==1 && !g.invalid_range && !g.overlaps);assert(g.first.offset==0 && g.first.size==4096 && g.first.host0==2 && g.first.host1==3);++groups;
    fixture();row(0,4096,0,3,4);g=inspect();assert(g.mask==1 && g.partial_empty==1 && g.first.size==0);++groups;
    fixture();row(1,SMEM_BYTES-4096,4096,2,3);g=inspect();assert(g.mask==2 && g.invalid_range==1 && g.first_index==1);++groups;
    fixture();row(1,0xfffffff0,4096,2,3);g=inspect();assert(g.mask==2 && g.first_cause==2);++groups;
    fixture();row(2,0x11e000,4096,2,3);g=inspect();assert(g.mask==4 && g.overlaps==1 && g.first_index==2);++groups;
    fixture();row(0,0x11c000,4096,2,3);row(1,0x199000,4096,3,4);g=inspect();assert(!g.mask);++groups;
    fixture();g=inspect();assert(!g.overlaps);++groups;
    fixture();row(0,0,4096,2,3);row(1,SMEM_BYTES,4096,3,4);row(2,0x11e000,4096,4,5);
    g=inspect();assert(g.mask==7 && g.partial_empty==1 && g.invalid_range==1 && g.overlaps==1 && g.first_index==0);++groups;
    fixture();row(0,0,0,2,3);g=inspect();assert(!g.mask);++groups;
    fixture();assert(capture_layout(&layout_first)==0);assert(capture_layout(&layout_second)==0);assert(layout_matches(&layout_first,&layout_second));
    row(0,0,4096,2,3);assert(capture_layout(&layout_second)==0);assert(!layout_matches(&layout_first,&layout_second));++groups;
    fixture();assert(capture_layout(&layout_first)==0);assert(capture_layout(&layout_second)==0);
    ++layout_second.rows[0].host1;assert(!layout_matches(&layout_first,&layout_second));
    layout_second=layout_first;++layout_second.rows[0].cacheline;assert(!layout_matches(&layout_first,&layout_second));
    layout_second=layout_first;++layout_second.item_limit;assert(!layout_matches(&layout_first,&layout_second));
    layout_second=layout_first;--layout_second.count;assert(!layout_matches(&layout_first,&layout_second));++groups;
    fixture();u32(SMEM_BYTES-4096+32+8*48,0);g=inspect();assert(!g.mask && layout_first.item_limit==512);++groups;
    fixture();u32(SMEM_BYTES-4096+8,0xffffffff);assert(read_layout_once()==-4 && census_release_sent==1 && releases==1);++groups;
    fixture();u32(192,0);assert(read_layout_once()==-1 && census_release_sent==1 && releases==1);++groups;
    fixture();foreign=1;owner=2;assert(read_layout_once()==-7 && !releases && !census_release_sent && !census_scans && claims==HWLOCK_TRIES);++groups;
    fixture();lose_on_release=1;assert(read_layout_once()==-31 && !releases && !census_release_sent && owner==2);++groups;
    assert(munmap(image,SMEM_BYTES)==0);free(before);
    printf("%u C layout fixture groups passed; full SMEM snapshot was VM read-only, payload scalar reads rejected, and empty/range/overlap causes remained distinct.\n",groups);
    return 0;
}
'''

if __name__ == "__main__":
    clang = shutil.which("clang")
    if not clang:
        raise SystemExit("clang is required")
    with tempfile.TemporaryDirectory(prefix="ipa-smem-layout-") as name:
        temporary = Path(name)
        (temporary / "microkit.h").write_text(census.STUB)
        (temporary / "test.c").write_text(TEST.replace("LAYOUT_SOURCE", json.dumps(str(HERE / "ipa-smem-layout.c"))))
        subprocess.run([clang, "-O2", "-Wall", "-Wextra", "-Werror", "-I", str(temporary), str(temporary / "test.c"), "-o", str(temporary / "test")], check=True)
        subprocess.run([str(temporary / "test")], check=True)
