#!/usr/bin/env python3
"""Exercise the actual C metadata parser on synthetic, guarded host memory."""
from pathlib import Path
import json
import shutil
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
STUB = """#include <stdint.h>
typedef uint32_t microkit_channel;
static inline void microkit_dbg_putc(char c) {(void)c;}
static inline void microkit_dbg_puts(const char *s) {(void)s;}
"""
TEST = r'''
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#define SMEM_PROBE_HOST_TEST 1
#include CENSUS_SOURCE
static uint8_t *image;
static void u16(uint32_t at,uint16_t value) {memcpy(image+at,&value,2);}
static void u32(uint32_t at,uint32_t value) {memcpy(image+at,&value,4);}
static void fixture(int global) {
    assert(mprotect(image,SMEM_BYTES,PROT_READ|PROT_WRITE)==0);
    memset(image,0,SMEM_BYTES);smem_vaddr=(uintptr_t)image;
    u32(192,1);u32(92,12u<<16);
    uint32_t table=SMEM_BYTES-4096;
    memcpy(image+table,"$TOC",4);u32(table+4,1);u32(table+8,1);
    uint32_t entry=table+32;
    u32(entry,0x10000);u32(entry+4,0x10000);
    u16(entry+12,global?0xfffe:0);u16(entry+14,global?0xfffe:1);u32(entry+16,64);
    memcpy(image+0x10000,"$PRT",4);u16(0x10004,global?0xfffe:0);u16(0x10006,global?0xfffe:1);
    u32(0x10008,0x10000);u32(0x1000c,0x6000);u32(0x10010,0x10000);
    u16(0x10020,0xa5a5);u16(0x10022,497);u32(0x10024,8192);u16(0x1002a,16336);
}
static int scan(struct item_result *item,struct route_result *route) {
    memset(item,0,sizeof(*item));memset(route,0,sizeof(*route));return find_items(item,route);
}
int main(void) {
    assert(sysconf(_SC_PAGESIZE)<=16384);
    image=mmap(NULL,SMEM_BYTES,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANON,-1,0);assert(image!=MAP_FAILED);
    struct item_result item,second;struct route_result route,second_route;
    fixture(0);
    assert(mprotect(image+0x14000,16384,PROT_NONE)==0);
    assert(scan(&item,&route)==0);assert(item.found==1 && item.offset==0x14000 && item.size==8192 && item.cached==0);
    assert(route.global_fallback==0 && route.offset==0x10000 && route.free_uncached==0x6000);
    assert(scan(&second,&second_route)==0);assert(second.offset==item.offset && second.size==item.size);
    /* Absent item walks over the protected payload without reading it. */
    u16(0x10022,496);assert(scan(&item,&route)==0 && item.found==0);
    fixture(1);assert(scan(&item,&route)==0 && item.found && route.global_fallback==1);
    fixture(0);u32(204,1);assert(scan(&item,&route)==-1);
    fixture(0);u32(92,11u<<16);assert(scan(&item,&route)==-2);
    fixture(0);u32(SMEM_BYTES-4096+8,0xffffffff);assert(scan(&item,&route)==-4);
    fixture(0);u32(SMEM_BYTES-4096+32,SMEM_BYTES-4096);assert(scan(&item,&route)==-9);
    fixture(0);
    uint32_t table=SMEM_BYTES-4096;
    u32(table+8,2);memcpy(image+table+32+48,image+table+32,48);assert(scan(&item,&route)==-5);
    fixture(0);u16(0x10020,0);assert(scan(&item,&route)==-20);
    fixture(0);u16(0x10028,8192);assert(scan(&item,&route)==-20);
    fixture(0);u32(table+32+4,0x8040);u32(0x10008,0x8040);
    u32(0x1000c,32);u32(0x10010,0x3fc0);
    u16(0x18000,0xa5a5);u16(0x18002,497);u32(0x18004,16384);u16(0x18008,8192);
    assert(mprotect(image+0x14000,16384,PROT_NONE)==0);
    assert(scan(&item,&route)==0 && item.found && item.cached==1 && item.offset==0x14000 && item.size==8192);
    u16(0x18000,0);assert(scan(&item,&route)==-20);
    fixture(0);u32(0x1000c,32);u32(0x10010,0xdf80);u32(table+32+16,3);
    assert(scan(&item,&route)==-20);
    assert(munmap(image,SMEM_BYTES)==0);
    puts("13 C fixture groups passed; protected SMEM item payloads were never read.");
    return 0;
}
'''

if __name__ == "__main__":
    clang = shutil.which("clang")
    if not clang: raise SystemExit("clang is required")
    with tempfile.TemporaryDirectory(prefix="ipa-smem-census-") as name:
        temporary=Path(name)
        (temporary/"microkit.h").write_text(STUB)
        code=TEST.replace("CENSUS_SOURCE",json.dumps(str(HERE/"ipa-smem-census.c")))
        (temporary/"test.c").write_text(code)
        subprocess.run([clang,"-O2","-Wall","-Wextra","-Werror","-I",str(temporary),str(temporary/"test.c"),"-o",str(temporary/"test")],check=True)
        subprocess.run([str(temporary/"test")],check=True)
