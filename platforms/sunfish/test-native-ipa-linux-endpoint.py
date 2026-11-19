#!/usr/bin/env python3
"""Offline AP endpoint tests against compiled, pinned Linux setup functions.

No device commands. The oracle compiles the actual endpoint declarations,
v4.2 register descriptors, configuration initializer and setup/program
functions. Kernel-only facilities and MMIO use host fixtures. The SKB/QMAP
layout is independently compiled from the pinned declarations; its thirteen
constants were also cross-checked with the exact reference kernel compiler,
headers and configuration (without executing any probe on a device).
"""
from __future__ import annotations
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
PIN = "2267e248204e7e051e0f7355298949a6440de579"
ARCHIVE_HASH = "d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2"
CONFIG_HASH = "17ac093aabdca99e0ffa96d2670d71447f461f8fc5f625d6913829b4f79acef4"
ARCHIVE = ROOT / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
CONFIG = ROOT / ".cache/sunfish-mobian-comparison/artifacts/kernel.config"
OUT = ROOT / ".cache/sunfish-native-wifi/private/ipa-linux-endpoint"
IPA = "drivers/net/ipa/"
INPUTS = [IPA+n for n in ("ipa_endpoint.c", "ipa_endpoint.h", "ipa_reg.h",
    "reg/ipa_reg-v4.2.c", "data/ipa_data-v4.2.c", "ipa_data.h", "reg.h", "ipa_version.h", "ipa_main.c")]
INPUTS += ["include/linux/if_rmnet.h", "include/linux/skbuff.h", "include/linux/cache.h",
    "arch/arm64/include/asm/cache.h", "include/uapi/linux/if_ether.h", "include/vdso/cache.h"]
ABI_NAMES = ["page_bytes", "cache_bytes", "net_skb_pad", "skb_shared_info_bytes",
    "skb_shared_info_aligned", "ipa_rx_overhead", "mtu", "aggregation_kb",
    "qmap_header_bytes", "qmap_mux_offset", "qmap_length_offset",
    "qmap_ul_checksum_bytes", "qmap_v5_checksum_bytes"]
EXPECTED_ABI = [4096,64,64,320,320,384,1500,6,4,1,2,4,4]

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def run(args, name, env=None, expected=0):
    r=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(name+".log")).write_text(r.stdout+r.stderr)
    if r.returncode != expected: raise SystemExit(f"{name}: exit {r.returncode}; see {OUT/(name+'.log')}")
    return r
def definition(text, pattern):
    m=re.search(pattern,text,re.M);assert m,pattern
    start=m.start();i=text.index("{",m.end()-1);depth=1;i+=1
    while depth:
        if text[i]=='{':depth+=1
        elif text[i]=='}':depth-=1
        i+=1
    return text[start:text.index(';',i)+1]+"\n"
def macro(text,name):
    lines=text.splitlines()
    for i,line in enumerate(lines):
        if re.match(r'^#define\s+'+re.escape(name)+r'(?:\(|\s|$)',line):
            result=[line]
            while result[-1].rstrip().endswith('\\'):
                i+=1;result.append(lines[i])
            return '\n'.join(result)+'\n'
    raise AssertionError(name)
def oracle(source, config):
    spec=importlib.util.spec_from_file_location('gsi_oracle_helpers',Path(__file__).with_name('test-native-ipa-linux-gsi.py'))
    h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
    endpoint=source[IPA+'ipa_endpoint.c'];skb=source['include/linux/skbuff.h']
    data=source[IPA+'data/ipa_data-v4.2.c']
    prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
typedef uint8_t u8; typedef uint8_t __u8;
typedef uint16_t u16; typedef uint16_t __be16; typedef uint16_t __sum16;
typedef uint32_t u32; typedef uint64_t u64; typedef uint64_t __u64;
typedef int64_t ktime_t; typedef unsigned long netmem_ref;
typedef struct { int counter; } atomic_t;
#define __aligned(n) __attribute__((aligned(n)))
#define __force
#define BIT(n) (UINT32_C(1) << (n))
#define GENMASK(h,l) ((UINT32_MAX >> (31-(h))) & (UINT32_MAX << (l)))
#define ARRAY_SIZE(a) (sizeof(a)/sizeof((a)[0]))
#define ALIGN(n,a) (((n)+(a)-1)&~((a)-1))
#define max(x,y) ((x)>(y)?(x):(y))
#define hweight32(v) __builtin_popcount(v)
#define __ffs(v) __builtin_ctz(v)
#define ilog2(v) (63 - __builtin_clzll(v))
#define DIV_ROUND_CLOSEST(n,d) (((n)+(d)/2)/(d))
#define SZ_1K 1024
#define cpu_to_be32(v) __builtin_bswap32(v)
#define DECLARE_BITMAP(name,bits) unsigned long name[1]
#define for_each_set_bit(i,map,count) for ((i)=0;(i)<(count);(i)++) if ((map)[0] & (1UL<<(i)))
#define clear_bit(i,map) ((map)[0] &= ~(1UL<<(i)))
#define __set_bit(i,map) ((map)[0] |= 1UL<<(i))
#define INIT_DELAYED_WORK(work,fn) ((void)(work),(void)(fn))
struct delayed_work { int unused; };
static bool warn_on(bool c) { if(c) abort(); return false; }
static bool is_power_of_2(u32 v) { return v && !(v & (v-1)); }
#define WARN_ON(c) warn_on(c)
#define WARN(c,...) warn_on(c)
_Static_assert(sizeof(void*)==8 && sizeof(unsigned long)==8, "64-bit ABI required");
#if __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
#error Little endian oracle host required
#endif
'''
    layouts=[definition(skb,r'^typedef struct skb_frag\s*\{'),
        definition(skb,r'^struct skb_shared_hwtstamps\s*\{'),
        definition(skb,r'^struct xsk_tx_metadata_compl\s*\{'),
        definition(skb,r'^struct skb_shared_info\s*\{')]
    structs=r'''
struct gsi_channel { u8 trans_tre_max; };
struct gsi { struct gsi_channel channel[28]; };
struct ipa {
 enum ipa_version version; void *reg_virt; struct gsi gsi;
 struct ipa_endpoint endpoint[IPA_ENDPOINT_MAX];
 struct ipa_endpoint *name_map[IPA_ENDPOINT_COUNT];
 unsigned long defined[1], set_up[1]; u32 endpoint_count;
};
const struct reg *ipa_reg(struct ipa *ipa, enum ipa_reg_id id) {
 (void)ipa; return reg(&ipa_regs_v4_2,id);
}
static u8 mmio[0x7000];
static u32 write_count;
static void iowrite32(u32 value,void *address) {
 size_t off=(u8*)address-mmio;
 if(off>=4096 || off%4) abort();
 static const enum ipa_reg_id ids[]={ENDP_INIT_CFG,ENDP_INIT_NAT,ENDP_INIT_HDR,
  ENDP_INIT_HDR_EXT,ENDP_INIT_HDR_METADATA_MASK,ENDP_INIT_MODE,ENDP_INIT_AGGR,
  ENDP_INIT_HOL_BLOCK_EN,ENDP_INIT_DEAGGR,ENDP_INIT_RSRC_GRP,ENDP_INIT_SEQ,ENDP_STATUS};
 static const u32 endpoints[]={1,6,8,9};
 const struct reg *found=NULL;
 for(size_t i=0;i<ARRAY_SIZE(ids);i++) for(size_t j=0;j<ARRAY_SIZE(endpoints);j++) {
  const struct reg *r=reg(&ipa_regs_v4_2,ids[i]);
  if(reg_n_offset(r,endpoints[j])==off) { if(found) abort();found=r; }
 }
 if(!found) abort();
 u32 mask=found->fcount?0:UINT32_MAX;
 for(u32 i=0;i<found->fcount;i++) mask|=found->fmask[i];
 printf("%05lx %08x %08x\n",(unsigned long)off,value,mask);write_count++;
}
static bool ipa_endpoint_init_ctrl(struct ipa_endpoint *endpoint,bool enable) {
 (void)endpoint;(void)enable;abort();
}
static void ipa_endpoint_suspend_aggr(struct ipa_endpoint *endpoint) { (void)endpoint;abort(); }
static void ipa_endpoint_program_delay(struct ipa_endpoint *endpoint,bool enable) {
 (void)endpoint;(void)enable;abort();
}
static void ipa_endpoint_init_hol_block_enable(struct ipa_endpoint *endpoint,u32 us) {
 (void)endpoint;(void)us;abort();
}
static void ipa_endpoint_replenish_work(void) { abort(); }
'''
    main=r'''
int main(void) {
 printf("abi %lu %lu %lu %lu %lu %lu %u %u %lu %lu %lu %lu %lu\n",
  (unsigned long)PAGE_SIZE,(unsigned long)SMP_CACHE_BYTES,(unsigned long)NET_SKB_PAD,
  (unsigned long)sizeof(struct skb_shared_info),
  (unsigned long)SKB_DATA_ALIGN(sizeof(struct skb_shared_info)),
  (unsigned long)IPA_RX_BUFFER_OVERHEAD,IPA_MTU,
  ipa_aggr_size_kb(8192-NET_SKB_PAD,false),
  (unsigned long)sizeof(struct rmnet_map_header),
  (unsigned long)offsetof(struct rmnet_map_header,mux_id),
  (unsigned long)offsetof(struct rmnet_map_header,pkt_len),
  (unsigned long)sizeof(struct rmnet_map_ul_csum_header),
  (unsigned long)sizeof(struct rmnet_map_v5_csum_header));
 struct ipa ipa={.version=IPA_VERSION_4_2,.reg_virt=mmio,.endpoint_count=17};
 const struct reg *cfg=ipa_reg(&ipa,ENDP_INIT_CFG);
 if(reg_offset(cfg)!=0x808 || reg_decode(cfg,CS_OFFLOAD_EN,0xa)!=1) abort();
 for(u32 name=0;name<ARRAY_SIZE(ipa_gsi_endpoint_data);name++) {
  const struct ipa_gsi_endpoint_data *d=&ipa_gsi_endpoint_data[name];
  if(!d->endpoint_id) continue;
  struct ipa_endpoint *e=&ipa.endpoint[d->endpoint_id];
  e->ipa=&ipa;e->ee_id=d->ee_id;e->channel_id=d->channel_id;
  e->endpoint_id=d->endpoint_id;e->toward_ipa=d->toward_ipa;e->config=d->endpoint.config;
  ipa.name_map[name]=e;ipa.defined[0]|=1UL<<e->endpoint_id;
  if(e->ee_id==GSI_EE_AP) ipa.gsi.channel[e->channel_id].trans_tre_max=d->channel.tlv_count;
 }
 if(ipa.defined[0]!=0xf72) abort();
 ipa_endpoint_setup(&ipa);
 if(write_count!=36 || ipa.set_up[0]!=0x342) abort();
}
'''
    functions=['ipa_aggr_size_kb','ipa_endpoint_program_suspend',
        'ipa_endpoint_init_cfg','ipa_endpoint_init_nat','ipa_qmap_header_size',
        'ipa_header_size_encode','ipa_metadata_offset_encode','ipa_endpoint_init_hdr',
        'ipa_endpoint_init_hdr_ext','ipa_endpoint_init_hdr_metadata_mask','ipa_endpoint_init_mode',
        'ipa_qtime_val','aggr_time_limit_encode','ipa_endpoint_init_aggr',
        'ipa_endpoint_init_hol_block_en','ipa_endpoint_init_hol_block_disable',
        'ipa_endpoint_init_deaggr','ipa_endpoint_init_rsrc_grp','ipa_endpoint_init_seq',
        'ipa_endpoint_status','ipa_endpoint_program','ipa_endpoint_setup_one','ipa_endpoint_setup']
    frags=int(re.search(r'^CONFIG_MAX_SKB_FRAGS=(\d+)$',config,re.M)[1])
    return '\n'.join([prefix,'#define PAGE_SIZE 4096',f'#define MAX_SKB_FRAGS {frags}',
        macro(source['arch/arm64/include/asm/cache.h'],'L1_CACHE_SHIFT'),
        macro(source['arch/arm64/include/asm/cache.h'],'L1_CACHE_BYTES'),
        h.strip_includes(source['include/vdso/cache.h']), *layouts,
        *(macro(skb,n) for n in ['SKB_DATA_ALIGN','SKB_WITH_OVERHEAD','SKB_MAX_ORDER','NET_SKB_PAD']),
        h.strip_includes(source['include/linux/if_rmnet.h']),
        macro(source['include/uapi/linux/if_ether.h'],'ETH_DATA_LEN'),
        h.strip_includes(source[IPA+'reg.h']), h.strip_includes(source[IPA+'ipa_reg.h']),
        h.strip_includes(source[IPA+'ipa_version.h']),h.strip_includes(source[IPA+'ipa_endpoint.h']),
        h.strip_includes(source[IPA+'ipa_data.h']),h.strip_includes(source[IPA+'reg/ipa_reg-v4.2.c']),
        definition(data,r'^enum ipa_rsrc_group_id\s*\{'),
        definition(data,r'^static const struct ipa_gsi_endpoint_data ipa_gsi_endpoint_data\[\]\s*=\s*\{'),
        macro(endpoint,'IPA_RX_BUFFER_OVERHEAD'),macro(endpoint,'IPA_ENDPOINT_QMAP_METADATA_MASK'),
        structs,*(h.function(endpoint,n) for n in functions),main])

def main():
    OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    assert sha(ARCHIVE)==ARCHIVE_HASH and sha(CONFIG)==CONFIG_HASH
    source={}
    with tarfile.open(ARCHIVE) as t:
        prefix=t.getnames()[0].split('/')[0]+'/'
        for rel in INPUTS:
            body=t.extractfile(prefix+rel).read();p=OUT/'linux-source'/rel
            p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(body);source[rel]=body.decode()
    # Bind the later DMA work to the exact source ordering: exception RX is
    # enabled only after the command endpoint's memory/table initialization.
    spec=importlib.util.spec_from_file_location('endpoint_startup_helpers',Path(__file__).with_name('test-native-ipa-linux-gsi.py'))
    helper=importlib.util.module_from_spec(spec);spec.loader.exec_module(helper)
    startup=helper.function(source[IPA+'ipa_main.c'],'ipa_setup')
    startup_order=['gsi_setup(&ipa->gsi)','ipa_endpoint_setup(ipa)',
        'ipa_endpoint_enable_one(command_endpoint)','ipa_mem_setup(ipa)',
        'ipa_table_setup(ipa)','ipa_endpoint_enable_one(exception_endpoint)',
        'ipa_endpoint_default_route_set(ipa, exception_endpoint->endpoint_id)','ipa_qmi_setup(ipa)']
    locations=[startup.index(call) for call in startup_order]
    assert locations==sorted(set(locations)), 'Linux startup order changed'
    c=OUT/'linux-endpoint-oracle.c';c.write_text(oracle(source,CONFIG.read_text()))
    cc=shutil.which('clang') or shutil.which('cc');assert cc
    run([cc,'-std=gnu11','-Wall','-Wextra','-Werror',str(c),'-o',str(OUT/'linux-endpoint-oracle')],'c-build')
    lines=run([str(OUT/'linux-endpoint-oracle')],'c-oracle').stdout.splitlines()
    abi=[int(n) for n in lines[0].split()[1:]];assert abi==EXPECTED_ABI
    exact=OUT/'reference-abi.json'
    if exact.exists():assert dict(zip(ABI_NAMES,abi))==json.loads(exact.read_text())
    assert len(lines[1:])==36
    golden=OUT/'linux-endpoint-writes.txt';golden.write_text('\n'.join(lines[1:])+'\n')
    # Reuse the GSI behavioral fixture unchanged, with its modules bound to
    # this test crate's actual core types. The endpoint groups run separately.
    fixture=(ROOT/'tests/sunfish_ipa_linux_gsi_test.rs').read_text()
    fixture='use crate::{sunfish_ipa_linux_buffers,sunfish_ipa_linux_gsi};\n'+fixture[fixture.index('use std::collections'):]
    fixture+='\npub fn completed_setup() -> SetupOnce { let mut once=SetupOnce::new(setup_plan(),50).unwrap(); setup_run(&mut once,&mut Fake::cold()).unwrap();once }\n'
    fixture+='pub fn incomplete_setup() -> SetupOnce { SetupOnce::new(setup_plan(),50).unwrap() }\n'
    f=OUT/'gsi-fixture.rs';f.write_text(fixture)
    env=dict(os.environ,SUNFISH_ENDPOINT_GSI_FIXTURE=str(f),SUNFISH_ENDPOINT_C_REFERENCE=str(golden))
    rustc=shutil.which('rustc');assert rustc
    binary=OUT/'endpoint-test'
    run([rustc,'--edition=2024','-D','warnings','--test',str(ROOT/'tests/sunfish_ipa_linux_endpoint_test.rs'),'-o',str(binary)],'rust-build',env)
    r=run([str(binary),'epcfg_'],'rust-tests',env)
    groups=int(re.search(r'test result: ok\. (\d+) passed;',r.stdout)[1])
    wrapper=OUT/'core.rs';names=['sunfish_ipa_linux_buffers','sunfish_ipa_linux_gsi','sunfish_ipa_linux_endpoint']
    wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names))
    run([rustc,'--edition=2024','-D','warnings','--crate-type=lib','--target=aarch64-unknown-none',str(wrapper),'-o',str(OUT/'endpoint-aarch64.rlib')],'aarch64-core')
    original=(ROOT/'rust/src/sunfish_ipa_linux_endpoint.rs').read_text()
    harness=(ROOT/'tests/sunfish_ipa_linux_endpoint_test.rs').read_text()
    mutations={'wrong-command-destination':('w(CommandTx, Mode, 0x83)','w(CommandTx, Mode, 0x93)'),
        'wrong-byte-limit':('AGGREGATION_KB: u32 = 6','AGGREGATION_KB: u32 = 7'),
        'wrong-metadata-endian':('w(ModemRx, MetadataMask, 0xff000000)','w(ModemRx, MetadataMask, 0x000000ff)'),
        'missing-header-fields':('Self::Header => 0x1fffffff','Self::Header => 0x0000003f')}
    for name,(before,after) in mutations.items():
        assert original.count(before)==1,(name,before)
        mutant=OUT/(name+'.rs');mutant.write_text(original.replace(before,after))
        test=OUT/(name+'-test.rs');body=harness
        for n in ['sunfish_ipa_linux_buffers','sunfish_ipa_linux_gsi']:
            body=body.replace('../rust/src/'+n+'.rs',str(ROOT/'rust/src'/(n+'.rs')))
        body=body.replace('../rust/src/sunfish_ipa_linux_endpoint.rs',str(mutant));test.write_text(body)
        executable=OUT/(name+'-test')
        run([rustc,'--edition=2024','-D','warnings','--test','--crate-name','endpoint_mutation',str(test),'-o',str(executable)],name+'-build',env)
        run([str(executable),'--exact','epcfg_profile_matches_actual_compiled_linux_setup_and_abi'],name,env,expected=101)
    selected=['rust/src/sunfish_ipa_linux_endpoint.rs','tests/sunfish_ipa_linux_endpoint_test.rs',
        'platforms/sunfish/test-native-ipa-linux-endpoint.py','tests/sunfish_ipa_linux_gsi_test.rs',
        'rust/src/sunfish_ipa_linux_buffers.rs','rust/src/sunfish_ipa_linux_gsi.rs',
        'platforms/sunfish/test-native-ipa-linux-gsi.py','platforms/sunfish/test-modem-qmi.py','rust/src/lib.rs']
    for n in selected:
        dest=OUT/'source'/n;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/n,dest)
    manifest={'linux_commit':PIN,'archive_sha256':ARCHIVE_HASH,'configuration_sha256':CONFIG_HASH,
        'reference_sha256':{n:sha(OUT/'linux-source'/n) for n in INPUTS},
        'startup_order':startup_order,
        'source_sha256':{n:sha(ROOT/n) for n in selected},'abi':dict(zip(ABI_NAMES,abi)),
        'compiled_c_writes':36,'encoder_mutations_rejected':len(mutations),'rust_behavioral_groups':groups,'aarch64_no_std':True,
        'device_actions':[],'limits':'Offline endpoint configuration core; no native backend, channel start, transfers or production Ready.'}
    manifest['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)} for p in OUT.iterdir() if p.is_file() and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'PASS: {groups} endpoint groups, 36 exact Linux C writes/masks, {len(mutations)} rejected mutations, derived SKB/QMAP ABI, AArch64 no_std; {OUT}')

if __name__=='__main__':main()
