#!/usr/bin/env python3
"""Offline first IPA memory transaction, using compiled pinned Linux functions.

The original ipa_mem_setup(), command payload encoders, TRE encoders, register
descriptors and ring doorbells are compiled unchanged. Allocator, DMA commit
and MMIO are host stand-ins, not evidence of hardware or native DMA authority.
"""
from pathlib import Path
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
OUT = ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-memsetup'
IPA = 'drivers/net/ipa/'
INPUTS = [IPA + n for n in ('ipa_mem.c', 'ipa_mem.h', 'ipa_cmd.c', 'ipa_cmd.h',
    'gsi.c', 'gsi_trans.c', 'reg.h', 'ipa_reg.h', 'gsi_reg.h',
    'reg/ipa_reg-v4.2.c', 'reg/gsi_reg-v4.0.c', 'data/ipa_data-v4.2.c')]


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(args, name, env=None, expected=0):
    p = subprocess.run(args, cwd=ROOT, capture_output=True, text=True, env=env)
    (OUT / (name + '.log')).write_text(p.stdout + p.stderr)
    if p.returncode != expected:
        raise SystemExit(f'{name}: exit {p.returncode}; see {OUT / (name + ".log")}')
    return p


def definition(s, pattern):
    m = re.search(pattern, s, re.M)
    assert m, pattern
    start = m.start()
    end = s.index('{', m.end() - 1) + 1
    depth = 1
    while depth:
        if s[end] == '{': depth += 1
        if s[end] == '}': depth -= 1
        end += 1
    return s[start:end]


def function(s, name):
    return definition(s, r'^(?:static\s+)?(?:void|int|u32|__le16|__le32|const struct ipa_mem \*)\s*' + re.escape(name) + r'\(') + '\n'


def oracle(source, g):
    cmd = source[IPA + 'ipa_cmd.c']
    trans = source[IPA + 'gsi_trans.c']
    c = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
typedef uint8_t u8; typedef uint16_t u16; typedef uint32_t u32; typedef uint64_t u64;
typedef u16 __le16; typedef u32 __le32; typedef u64 __le64; typedef u64 dma_addr_t;
#if __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
#error Little endian oracle host required
#endif
#define cpu_to_le16(v) ((u16)(v))
#define cpu_to_le32(v) ((u32)(v))
#define cpu_to_le64(v) ((u64)(v))
#define lower_32_bits(v) ((u32)(v))
#define ARRAY_SIZE(a) (sizeof(a)/sizeof((a)[0]))
#define GENMASK(h,l) ((UINT32_MAX >> (31-(h))) & (UINT32_MAX << (l)))
#define BIT(n) (UINT32_C(1) << (n))
#define __ffs(v) __builtin_ctz(v)
#define __fls(v) (31 - __builtin_clz(v))
#define u32_encode_bits(v,m) (((u32)(v) << __builtin_ctz(m)) & (m))
#define container_of(p,t,m) ((t*)((u8*)(p)-offsetof(t,m)))
#define GSI_EE_AP 0
#define GSI_RING_ELEMENT_SIZE 16
#define U16_MAX UINT16_MAX
#define dev_err(dev,...) ((void)(dev))
static bool warning(bool c) { if(c) abort(); return false; }
#define WARN_ON(c) warning(c)
#define WARN(c,...) warning(c)
static bool is_power_of_2(u32 v) { return v && !(v&(v-1)); }
'''
    c += '\n'.join(g.strip_includes(source[IPA+n]) for n in
        ('reg.h', 'ipa_reg.h', 'gsi_reg.h', 'ipa_mem.h', 'ipa_cmd.h'))
    # The two original translation units use the same private array name.
    # Rename only that private symbol while combining them in one host TU.
    for name, symbol in [('reg/ipa_reg-v4.2.c','ipa_reg_array'),('reg/gsi_reg-v4.0.c','gsi_reg_array')]:
        c += '\n#define reg_array ' + symbol + '\n' + g.strip_includes(source[IPA+name]) + '\n#undef reg_array\n'
    c += definition(source[IPA+'data/ipa_data-v4.2.c'], r'^static const struct ipa_mem ipa_mem_local_data\[\]\s*=\s*\{') + ';\n'
    for name in ('ipa_cmd_hw_hdr_init_local', 'ipa_cmd_hw_dma_mem_mem'):
        c += definition(cmd, r'^struct ' + name + r'\s*\{') + ';\n'
    c += 'union ipa_cmd_payload { struct ipa_cmd_hw_hdr_init_local hdr_init_local; struct ipa_cmd_hw_dma_mem_mem dma_shared_mem; u8 allocation[32]; };\n'
    for s, pattern in ((trans,r'^enum gsi_tre_type\s*\{'), (trans,r'^struct gsi_tre\s*\{')):
        c += definition(s, pattern) + ';\n'
    c += definition(source[IPA+'gsi.c'], r'^struct gsi_event\s*\{') + ';\n'
    for s, prefix in ((cmd,'HDR_INIT_LOCAL_FLAGS_'),(cmd,'DMA_SHARED_MEM_FLAGS_DIRECTION_'),(trans,'TRE_FLAGS_')):
        c += '\n'.join(line for line in s.splitlines() if line.startswith('#define '+prefix)) + '\n'
    c += r'''
struct gsi_ring { u64 addr; u32 count, index; };
struct gsi;
struct gsi_channel { struct gsi *gsi; struct gsi_ring tre_ring; u32 id; };
struct gsi_evt_ring { struct gsi_ring ring; };
struct gsi { void *virt; struct gsi_evt_ring evt_ring[1]; };
struct ipa { void *dev,*reg_virt; struct gsi gsi; const struct ipa_mem *mem; u32 mem_count,mem_offset; u64 zero_addr; };
struct entry { u32 size; u64 address; u16 opcode; u8 payload[32]; };
struct gsi_trans { struct gsi *gsi; u32 count; struct entry entries[4]; };
static u8 mmio[0x20000];
static u32 allocations,commits,writes;
static union ipa_cmd_payload payloads[4];
static struct gsi_trans transaction;
static u32 gsi_channel_id(struct gsi_channel *c) { return c->id; }
const struct reg *ipa_reg(struct ipa *i,enum ipa_reg_id id) { (void)i; return reg(&ipa_regs_v4_2,id); }
const struct reg *gsi_reg(struct gsi *g,enum gsi_reg_id id) { (void)g; return reg(&gsi_regs_v4_0,id); }
static void iowrite32(u32 v,void *a) {
 size_t offset=(u8*)a-mmio;
 printf("write %05zx %08x\n",offset,v);writes++;
}
static union ipa_cmd_payload *ipa_cmd_payload_alloc(struct ipa *i,dma_addr_t *addr) {
 (void)i;if(allocations>=4) abort();
 *addr=UINT64_C(0x80006000)+32*allocations;
 return &payloads[allocations++];
}
struct gsi_trans *ipa_cmd_trans_alloc(struct ipa *i,u32 count) {
 if(count!=4 || transaction.count) abort();transaction.gsi=&i->gsi;return &transaction;
}
static void gsi_trans_cmd_add(struct gsi_trans *t,void *payload,u32 size,u64 addr,enum ipa_cmd_opcode op) {
 if(t->count>=4 || size!=16) abort();
 struct entry *e=&t->entries[t->count++];e->size=size;e->address=addr;e->opcode=op;
 memcpy(e->payload,payload,32);
}
static void dump_bytes(const void *p,size_t n) { const u8 *b=p;for(size_t j=0;j<n;j++) printf("%02x",b[j]); }
'''
    c += '\n'.join(function(trans,n) for n in ('gsi_tre_len_opcode','gsi_tre_flags','gsi_trans_tre_fill'))
    c += '\n'.join(function(source[IPA+'gsi.c'],n) for n in ('gsi_ring_addr','gsi_evt_ring_doorbell','gsi_channel_doorbell'))
    c += r'''
static void gsi_trans_commit_wait(struct gsi_trans *t) {
 if(t->count!=4 || commits++) abort();
 for(u32 j=0;j<t->count;j++) {
  struct entry *e=&t->entries[j];struct gsi_tre tre;
  gsi_trans_tre_fill(&tre,e->address,e->size,j==t->count-1,true,e->opcode);
  printf("command %u %04x %08llx ",j,e->opcode,(unsigned long long)e->address);
  dump_bytes(e->payload,32);printf(" ");dump_bytes(&tre,16);printf("\n");
 }
 struct gsi_channel ch={.gsi=t->gsi,.id=1,.tre_ring={.addr=UINT64_C(0x80004000),.count=256,.index=4}};
 gsi_channel_doorbell(&ch);
 gsi_evt_ring_doorbell(t->gsi,0,1);
}
'''
    c += function(cmd,'ipa_cmd_hdr_init_local_add') + function(cmd,'ipa_cmd_dma_shared_mem_add')
    c += '\n'.join(function(source[IPA+'ipa_mem.c'],n) for n in ('ipa_mem_find','ipa_mem_zero_region_add','ipa_mem_setup'))
    c += r'''
int main(void) {
 struct ipa i={.reg_virt=mmio,.gsi={.virt=mmio,.evt_ring={{.ring={.addr=UINT64_C(0x80003000),.count=256}}}},
  .mem=ipa_mem_local_data,.mem_count=ARRAY_SIZE(ipa_mem_local_data),.zero_addr=UINT64_C(0x80000000)};
 const struct reg *r=ipa_reg(&i,LOCAL_PKT_PROC_CNTXT);
 if(reg_bit(gsi_reg(&i.gsi,GSI_STATUS),ENABLED)!=1 || reg_field_max(r,IPA_BASE_ADDR)!=0x1ffff ||
    reg_decode(r,IPA_BASE_ADDR,0x5f0)!=0x5f0) abort();
 if(ipa_mem_setup(&i) || allocations!=4 || commits!=1 || writes!=3) abort();
 struct gsi_event event={.xfer_ptr=UINT64_C(0x80004030),.len=0x80a1,.reserved1=0x73,
  .code=0x51,.reserved2=0x8495,.type=0x62,.chid=1};
 if(sizeof(event)!=16) abort();
 printf("event %08x ",UINT32_C(0x80003010));dump_bytes(&event,sizeof(event));printf("\n");
}
'''
    return c


def main():
    OUT.mkdir(mode=0o700,parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('memsetup_reference',HERE/'test-native-ipa-linux-gsi.py')
    g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g)
    assert sha(g.ARCHIVE)==g.ARCHIVE_HASH
    source={}
    with tarfile.open(g.ARCHIVE) as t:
        prefix=t.getnames()[0].split('/')[0]+'/'
        for rel in INPUTS:
            b=t.extractfile(prefix+rel).read()
            path=OUT/'linux-source'/rel;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(b)
            source[rel]=b.decode()
    c=OUT/'linux-memsetup-oracle.c';c.write_text(oracle(source,g))
    cc=shutil.which('clang') or shutil.which('cc');assert cc
    run([cc,'-std=gnu11','-Wall','-Wextra','-Werror',str(c),'-o',str(OUT/'linux-memsetup-oracle')],'c-build')
    golden=OUT/'linux-memsetup.txt';golden.write_text(run([str(OUT/'linux-memsetup-oracle')],'c-oracle').stdout)
    env=dict(os.environ,SUNFISH_MEMSETUP_REFERENCE=str(golden))
    rustc=shutil.which('rustc');assert rustc
    test=ROOT/'tests/sunfish_ipa_linux_memsetup_test.rs';binary=OUT/'memsetup-test'
    run([rustc,'--edition=2024','-D','warnings','--test',str(test),'-o',str(binary)],'rust-build',env)
    tested=run([str(binary)],'rust-tests',env)
    groups=int(re.search(r'test result: ok\. (\d+) passed;',tested.stdout)[1])
    wrapper=OUT/'core.rs'
    names=['sunfish_ipa_linux_config','sunfish_ipa_linux_memory','sunfish_ipa_linux_buffers','sunfish_ipa_linux_memsetup']
    wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names))
    run([rustc,'--edition=2024','-D','warnings','--crate-type=lib','--target=aarch64-unknown-none',str(wrapper),'-o',str(OUT/'memsetup-aarch64.rlib')],'aarch64-core')
    original=(ROOT/'rust/src/sunfish_ipa_linux_memsetup.rs').read_text()
    harness=test.read_text()
    mutations={'wrong-opcode':('0x13','0x12'),
        'wrong-direction':('c.payload[4..6].copy_from_slice','c.payload[6] = 1; c.payload[4..6].copy_from_slice'),
        'missing-bei':('COUNT { 0x600 }','COUNT { 0x200 }'),
        'last-tre-doorbell':('(COUNT * TRE_BYTES) as u64','((COUNT - 1) * TRE_BYTES) as u64')}
    for name,(before,after) in mutations.items():
        assert original.count(before)==1,(name,before)
        mutant=OUT/(name+'.rs');mutant.write_text(original.replace(before,after))
        body=harness.replace('../rust/src/sunfish_ipa_linux_memsetup.rs',str(mutant))
        for n in names[:-1]:body=body.replace('../rust/src/'+n+'.rs',str(ROOT/'rust/src'/(n+'.rs')))
        path=OUT/(name+'-test.rs');path.write_text(body);exe=OUT/(name+'-test')
        run([rustc,'--edition=2024','-D','warnings','--test','--crate-name','memsetup_mutation',str(path),'-o',str(exe)],name+'-build',env)
        run([str(exe),'--exact','memsetup_matches_compiled_linux_payloads_tres_and_doorbells'],name,env,expected=101)
    selected=['rust/src/'+n+'.rs' for n in names]+['rust/src/lib.rs','platforms/sunfish/test-modem-qmi.py',
        'tests/sunfish_ipa_linux_memsetup_test.rs','platforms/sunfish/test-native-ipa-linux-memsetup.py']
    for rel in selected:
        dest=OUT/'source'/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/rel,dest)
    manifest={'kernel_commit':g.PIN,'kernel_archive_sha256':g.ARCHIVE_HASH,
        'reference_sha256':{n:sha(OUT/'linux-source'/n) for n in source},
        'source_sha256':{n:sha(ROOT/n) for n in selected},'compiled_c_commands':4,'compiled_c_payload_bytes':128,
        'compiled_c_tre_bytes':64,'compiled_c_event_bytes':16,'compiled_c_register_writes':3,'encoder_mutations_rejected':len(mutations),
        'behavioral_groups':groups,'aarch64_no_std':True,'hardware_commands':[],
        'limits':'Offline encoder/parser only. No native publication path, DMA authority, completion proof or readiness.'}
    manifest['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)}
        for p in OUT.iterdir() if p.is_file() and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'PASS: {groups} memory-transaction groups, 4 compiled Linux commands / 192 payload+TRE bytes, 3 register writes, {len(mutations)} rejected mutations; {OUT}')


if __name__=='__main__': main()
