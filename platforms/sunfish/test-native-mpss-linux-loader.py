#!/usr/bin/env python3
"""Offline PAS4 loader comparison against original pinned Linux C functions.

Private firmware, compact metadata and staged RAM stay under mode-0700 output.
The C driver runs against CPU allocations and SCM recording stubs, never SMC.
This is a loader comparison, not native MPSS/GLINK/WLAN boot evidence.
"""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/mpss-linux-loader'
PIN = '2267e248204e7e051e0f7355298949a6440de579'
ARCHIVE_HASH = 'd55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'
FIRMWARE = ROOT / '.cache/sunfish-mobian-comparison/wifi-preparation/private/firmware/qcom/sm7150/google/sunfish/modem.mbn'
FIRMWARE_HASH = '8489e6828dc12ed98e26dec15adb7102a31aafaf7314e249854c737ddba4cdc0'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args, label, env=None, success=True):
    r = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)
    (OUT / (label+'.log')).write_text(r.stdout+r.stderr)
    if success and r.returncode:
        raise SystemExit(f'{label} failed; see {OUT / (label+".log")}')
    return r


def block(text, start):
    at = text.index(start)
    opening = text.index('{', at)
    end, depth = opening+1, 1
    while depth:
        depth += (text[end] == '{')-(text[end] == '}')
        end += 1
    if text[end:end+1] == ';': end += 1
    return text[at:end]+'\n'


def oracle():
    archive = ROOT / '.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive) != ARCHIVE_HASH or sha(FIRMWARE) != FIRMWARE_HASH:
        raise ValueError('Pinned archive or private firmware identity differs')
    names = ['drivers/soc/qcom/mdt_loader.c', 'drivers/firmware/qcom/qcom_scm.c',
             'drivers/firmware/qcom/qcom_scm.h', 'include/linux/firmware/qcom/qcom_scm.h',
             'drivers/remoteproc/qcom_q6v5_pas.c', 'drivers/remoteproc/qcom_q6v5.c',
             'arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts',
             'arch/arm64/boot/dts/qcom/sm7150.dtsi', 'include/uapi/linux/elf.h']
    originals = {}
    with tarfile.open(archive) as t:
        members = {m.name.split('/',1)[1]:m for m in t if '/' in m.name}
        for name in names:
            data = t.extractfile(members[name]).read()
            path = OUT/'linux-source'/name
            path.parent.mkdir(parents=True,exist_ok=True)
            path.write_bytes(data)
            originals[name] = data.decode()
    mdt, scm, header, api, pas, q6, board, soc, elf = (originals[n] for n in names)
    start = block(pas, 'static int qcom_pas_start(')
    in_order = ['qcom_q6v5_prepare(', 'qcom_pas_pds_enable(', 'clk_prepare_enable(',
                'qcom_mdt_pas_load(', 'qcom_scm_pas_prepare_and_auth_reset(', 'qcom_q6v5_wait_for_start(',
                'qcom_scm_pas_metadata_release(']
    positions = []
    for item in in_order:
        positions.append(start.index(item, positions[-1]+1 if positions else 0))
    assert positions == sorted(positions)
    assert 'msecs_to_jiffies(5000)' in start
    assert '.pas_id = 4,' in block(pas, 'static const struct qcom_pas_data mpss_resource_init =')
    assert '.dtb_pas_id' not in block(pas, 'static const struct qcom_pas_data mpss_resource_init =')
    assert '.load_state = "modem"' in block(pas, 'static const struct qcom_pas_data mpss_resource_init =')
    assert 'rproc->has_iommu = of_property_present(pdev->dev.of_node, "iommus");' in pas
    assert 'pas->pas_ctx->use_tzmem = rproc->has_iommu;' in pas
    node = block(board, '&remoteproc_mpss {')
    assert 'firmware-name = "qcom/sm7150/google/sunfish/modem.mbn";' in node
    # Board and SoC MPSS node have no host-controlled IOMMU for PAS loading.
    assert 'iommus' not in node
    assert 'reg = <0x0 0x8b000000 0x0 0x9800000>;' in board
    remote = block(soc, 'remoteproc_mpss: remoteproc@4080000 {')
    assert 'iommus' not in remote
    assert all(name in remote for name in ['"cx"','"mx"','"mss"','"xo"'])
    assert 'complete(&q6v5->start_done)' in block(q6, 'static irqreturn_t q6v5_ready_interrupt(')
    dtbs = []
    for relative in ['artifacts/sm7150-google-sunfish.dtb','device-runs/stage0-v2-ssh/live.dtb']:
        path=ROOT/'.cache/sunfish-mobian-comparison'/relative
        def prop(*args):
            return subprocess.check_output(['fdtget',str(path),*args],text=True,stderr=subprocess.DEVNULL).strip()
        node='/soc/remoteproc@4080000'
        assert 'iommus' not in prop('-p',node).split()
        assert prop('-t','s',node,'compatible')=='qcom,sm7150-mpss-pas'
        assert prop('-t','s',node,'firmware-name')=='qcom/sm7150/google/sunfish/modem.mbn'
        assert prop('-t','s',node,'status')=='okay'
        assert prop('-t','x','/reserved-memory/memory@8b000000','reg')=='0 8b000000 0 9800000'
        dtbs.append({'path':str(path),'sha256':sha(path),'host_iommu':False,'use_tzmem':False})
    c = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <assert.h>
#include <errno.h>
#include <limits.h>
#include <sys/types.h>
typedef uint32_t u32; typedef uint64_t u64; typedef uint64_t phys_addr_t;
typedef uint64_t dma_addr_t;
struct device {int unused;};
#define PHYS_ADDR_MAX UINT64_MAX
#define SZ_4K 4096
#define ALIGN(x,n) (((x)+(n)-1)&~((n)-1))
#define size_add(a,b) ((a)+(b))
#define size_mul(a,b) ((a)*(b))
#define QCOM_MDT_TYPE_MASK (7<<24)
#define QCOM_MDT_TYPE_HASH (2<<24)
#define QCOM_MDT_RELOCATABLE (1<<27)
#define GFP_KERNEL 0
#define kmalloc(n,f) malloc(n)
#define kfree free
#define ERR_PTR(e) ((void*)(intptr_t)(e))
#define PTR_ERR(p) ((intptr_t)(p))
#define IS_ERR(p) ((uintptr_t)(p)>=(uintptr_t)-4095)
#define dev_err(...) ((void)0)
#define ARM_SMCCC_OWNER_SIP 2
'''
    c += 'typedef uint16_t Elf32_Half; typedef uint32_t Elf32_Word,Elf32_Addr,Elf32_Off;\n'
    for macro in ['EI_NIDENT','ELFMAG','SELFMAG','PT_LOAD']:
        c += re.search(r'^#define\s+'+macro+r'\s+[^\n]+',elf,re.M)[0]+'\n'
    for structure, alias in [('elf32_hdr','Elf32_Ehdr'),('elf32_phdr','Elf32_Phdr'),('elf32_shdr','Elf32_Shdr')]:
        c += re.search(r'typedef struct '+structure+r' \{.*?\} '+alias+r';',elf,re.S)[0]+'\n'
    c += '_Static_assert(sizeof(Elf32_Ehdr)==52 && sizeof(Elf32_Phdr)==32 && sizeof(Elf32_Shdr)==40,"ELF32 ABI");\n'
    c += header
    c += block(api, 'struct qcom_scm_pas_context {')
    c += r'''
static struct device dev; static struct {struct device *dev;} scm_stub={&dev},*__scm=&scm_stub;
static struct qcom_scm_desc descriptors[3];static int descriptor_count,clocks,bandwidth;
static int fail_transport,fail_command,call_at;static unsigned char *metadata;static size_t metadata_len;
static int qcom_scm_clk_enable(void){assert(!clocks&&!bandwidth);clocks=1;return 0;}
static void qcom_scm_clk_disable(void){assert(clocks&&!bandwidth);clocks=0;}
static int qcom_scm_bw_enable(void){assert(clocks&&!bandwidth);bandwidth=1;return 0;}
static void qcom_scm_bw_disable(void){assert(clocks&&bandwidth);bandwidth=0;}
static int qcom_scm_call(void *d,const struct qcom_scm_desc *desc,struct qcom_scm_res *res){
 (void)d;assert(clocks&&bandwidth);assert(desc->svc==2&&desc->owner==2);assert(desc->args[0]==4);
 assert(descriptor_count<3);descriptors[descriptor_count++]=*desc;
 res->result[0]=call_at==descriptor_count?fail_command:0;
 return call_at==descriptor_count?fail_transport:0;
}
static void *dma_alloc_coherent(void *d,size_t n,dma_addr_t *pa,int flags){
 (void)d;(void)flags;assert(!metadata);*pa=0x81000000;metadata_len=n;metadata=calloc(1,n);assert(metadata);return metadata;
}
static void dma_free_coherent(void *d,size_t n,void *p,dma_addr_t pa){(void)d;(void)n;(void)p;(void)pa;assert(0);}
static int qcom_scm_pas_prep_and_init_image(struct qcom_scm_pas_context *ctx,const void *m,size_t n){
 (void)ctx;(void)m;(void)n;assert(0);return -EINVAL;
}
static int qcom_tzmem_shm_bridge_create(phys_addr_t p,size_t n,u64 *h){(void)p;(void)n;(void)h;assert(0);return -EINVAL;}
static void qcom_tzmem_shm_bridge_delete(u64 h){(void)h;assert(0);}
'''
    for sig in ['static int __qcom_scm_pas_init_image(', 'int qcom_scm_pas_init_image(',
                'int qcom_scm_pas_mem_setup(', 'int qcom_scm_pas_auth_and_reset(',
                'int qcom_scm_pas_prepare_and_auth_reset(']:
        c += block(scm, sig)
    c += r'''
struct firmware {size_t size; const unsigned char *data;};
static ssize_t mdt_load_split_segment(void *p,const struct elf32_phdr *h,unsigned int i,const char *n,struct device *d){
 (void)p;(void)h;(void)i;(void)n;(void)d;assert(0);return -EINVAL;
}
'''
    for sig in ['static bool mdt_header_valid(', 'static bool mdt_phdr_loadable(',
                'void *qcom_mdt_read_metadata(', 'static int __qcom_mdt_pas_init(',
                'static bool qcom_mdt_bins_are_split(', 'int qcom_mdt_load_no_init(',
                'int qcom_mdt_pas_load(']:
        c += block(mdt, sig)
    c += r'''
static void write_bytes(const char *p,const void *b,size_t n){FILE *f=fopen(p,"wb");assert(f);assert(fwrite(b,1,n,f)==n);assert(!fclose(f));}
int main(int argc,char **argv){
 assert(qcom_scm_remap_error(-1)==-EIO);
 assert(argc==5);FILE *f=fopen(argv[1],"rb");assert(f);assert(!fseek(f,0,SEEK_END));long n=ftell(f);assert(n==68460672);
 rewind(f);unsigned char *data=malloc(n);assert(data);assert(fread(data,1,n,f)==(size_t)n);assert(!fclose(f));
 struct firmware fw={n,data};unsigned char *target=malloc(0x9800000);assert(target);memset(target,0xa5,0x9800000);
 struct qcom_scm_pas_context ctx={.dev=&dev,.pas_id=4,.mem_phys=0x8b000000,.mem_size=0x9800000,.use_tzmem=false};
 phys_addr_t reloc=0;assert(qcom_mdt_pas_load(&ctx,&fw,"modem.mbn",target,&reloc)==0);
 assert(reloc==0x8b000000&&ctx.ptr==metadata&&ctx.size==9100&&metadata_len==9100);
 assert(descriptor_count==2);assert(qcom_scm_pas_prepare_and_auth_reset(&ctx)==0);assert(descriptor_count==3);
 assert(descriptors[0].cmd==1&&descriptors[0].arginfo==0x82&&descriptors[0].args[1]==0x81000000);
 assert(descriptors[1].cmd==2&&descriptors[1].arginfo==3&&descriptors[1].args[1]==0x8b000000&&descriptors[1].args[2]==0x9800000);
 assert(descriptors[2].cmd==5&&descriptors[2].arginfo==1);assert(!clocks&&!bandwidth);
 write_bytes(argv[2],metadata,metadata_len);write_bytes(argv[3],target,0x9800000);
 FILE *trace=fopen(argv[4],"w");assert(trace);
 for(int i=0;i<3;i++){struct qcom_scm_desc *d=&descriptors[i];
  fprintf(trace,"%llx %x",(unsigned long long)(0x40000000ULL|((u64)d->owner<<24)|(d->svc<<8)|d->cmd),d->arginfo);
  for(int a=0;a<6;a++)fprintf(trace," %llx",(unsigned long long)d->args[a]);fputc('\n',trace);
 }assert(!fclose(trace));
 // Original descriptor functions must propagate both error layers and balance resources.
 for(int op=0;op<3;op++)for(int which=0;which<2;which++){
  descriptor_count=0;call_at=1;fail_transport=which==0?-5:0;fail_command=which==1?9:0;
  struct qcom_scm_res res;int ret=op==0?__qcom_scm_pas_init_image(4,0x81000000,&res):op==1?qcom_scm_pas_mem_setup(4,0x8b000000,0x9800000):qcom_scm_pas_auth_and_reset(4);
  if(op==0&&!ret)ret=res.result[0];assert(ret==(which==0?-5:9));assert(!clocks&&!bandwidth);
 }
 free(target);free(data);free(metadata);puts("Original Linux MDT+PAS4 order, compact metadata, segments/BSS/gaps, SCM envelopes/status and direct-auth path passed in CPU RAM.");
}
'''
    (OUT/'linux-oracle.c').write_text(c)
    run(['cc','-std=gnu11','-O2','-Wall','-Wextra','-Werror','-Wno-sign-compare',str(OUT/'linux-oracle.c'),'-o',str(OUT/'linux-oracle')], 'c-build')
    run([str(OUT/'linux-oracle'),str(FIRMWARE),str(OUT/'metadata.bin'),str(OUT/'target.bin'),str(OUT/'pas-trace.txt')],'c-oracle')
    return {p:hashlib.sha256(t.encode()).hexdigest() for p,t in originals.items()}, dtbs


def main():
    os.umask(0o077)
    OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    if OUT.stat().st_mode & 0o077: raise ValueError('Output must be private mode 0700')
    selected = ['rust/src/sunfish_mpss_linux_loader.rs','rust/src/sunfish_mpss_linux_memory.rs',
                'rust/src/qcom_mdt.rs','rust/src/qcom_mdt_packed.rs','rust/src/sunfish_lpd.rs','rust/src/qcom_tftp.rs',
                'tests/sunfish_mpss_linux_loader_test.rs','tests/sunfish_mpss_linux_loader_cases.rs','tests/sunfish_mpss_linux_memory_test.rs',
                'platforms/sunfish/test-native-mpss-linux-loader.py','rust/src/lib.rs','rust/Cargo.toml','rust/Cargo.lock']
    hashes = {p:sha(ROOT/p) for p in selected}
    original_hashes, dtbs = oracle()
    env = dict(os.environ, SUNFISH_MPSS_FIRMWARE=str(FIRMWARE),SUNFISH_MPSS_C_METADATA=str(OUT/'metadata.bin'),
               SUNFISH_MPSS_C_TARGET=str(OUT/'target.bin'),SUNFISH_MPSS_C_TRACE=str(OUT/'pas-trace.txt'))
    flags = [shutil.which('rustc'),'--edition=2024','-D','warnings']
    harness = ROOT/'tests/sunfish_mpss_linux_loader_test.rs'
    run(flags+['--test',str(harness),'-o',str(OUT/'test')],'host-build',env)
    regular=run([str(OUT/'test')],'host-tests',env)
    run([str(OUT/'test'),'mpss_real_private','--ignored','--test-threads=1'],'private-image',env)
    original = (ROOT/selected[0]).read_text()
    mutants = {
        'ignore-secure-service-error': ('reply[1] != u64::from(r.kind == Kind::Availability)', 'false'),
        'ignore-monitor-error': ('if reply[0] != 0 {','if false {'),
        'skip-target-verification': ('Phase::VerifyTarget\n                            } else {','Phase::AuthAndReset\n                            } else {'),
        'omit-bss-clear': ('s.bytes.get(at + i).copied().unwrap_or(0)', 's.bytes.get(at + i).copied().unwrap_or(0xa5)'),
        'reverse-target-word': ('u32::from_le_bytes(value)', 'u32::from_be_bytes(value)'),
        'omit-scm-resources': ('let result = if r.kind == Kind::Availability {', 'let result = if true {'),
        'allow-unwind-replay': ('if self.busy\n            || self.report.error.is_some()', 'if self.report.error.is_some()'),
    }
    for label,(before,after) in mutants.items():
        if original.count(before)!=1: raise ValueError('Mutation not unique: '+label)
        folder=OUT/label;folder.mkdir(exist_ok=True)
        changed=original.replace(before,after).replace('"../../tests/sunfish_mpss_linux_loader_cases.rs"',json.dumps(str(ROOT/'tests/sunfish_mpss_linux_loader_cases.rs')))
        (folder/'loader.rs').write_text(changed)
        text=re.sub(r'#\[path = "([^"]+)"\]',lambda m:'#[path = '+json.dumps(str((ROOT/'tests'/m[1]).resolve()))+']',harness.read_text())
        text=text.replace(str(ROOT/selected[0]),str(folder/'loader.rs'))
        (folder/'test.rs').write_text(text)
        run(flags+['--test',str(folder/'test.rs'),'-o',str(folder/'test')],label+'-build',env)
        if run([str(folder/'test'),'mpss_'],label,env,success=False).returncode==0: raise ValueError('Fault accepted: '+label)
    wrapper=OUT/'native.rs'
    declarations='\n'.join(f'#[path="{ROOT}/rust/src/{name}.rs"]pub mod {name};' for name in
        ['qcom_mdt','qcom_mdt_packed','qcom_tftp','sunfish_lpd','sunfish_mpss_linux_loader','sunfish_mpss_linux_memory'])
    wrapper.write_text('#![no_std]\n'+declarations+r'''
use sunfish_mpss_linux_loader::{Backend,Error,Guard,Request,Phase,Loader};
pub struct Platform{pub guard:fn(Guard)->Result<(),Error>,pub acquire:fn()->Result<u64,Error>,pub invoke:fn(&Request)->Result<[u64;8],Error>,pub release:fn(u64)->Result<(),Error>}
impl Backend for Platform{type Lease=u64;fn guard(&mut self,s:Guard)->Result<(),Error>{(self.guard)(s)}fn acquire(&mut self)->Result<u64,Error>{(self.acquire)()}fn invoke(&mut self,r:&Request)->Result<[u64;8],Error>{(self.invoke)(r)}fn release(&mut self,l:u64)->Result<(),Error>{(self.release)(l)}}
pub fn step(l:&mut Loader<'_,sunfish_mpss_linux_memory::Window<'_>>,p:&mut Platform)->Result<Phase,Error>{l.step(p)}
''')
    run(flags+['--crate-type=lib','--target=aarch64-unknown-none','--emit=obj',str(wrapper),'-o',str(OUT/'native-aarch64.o')],'aarch64-native',env)
    sdk=ROOT/'.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    native_env=dict(env,SEL4_INCLUDE_DIRS=str(sdk/'board/sunfish/debug/include'),
                    CARGO_TARGET_DIR=str(ROOT/'.cache/sunfish-native-wifi/private/preflight-target'),
                    SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(ROOT/'.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo','+nightly-2026-09-25','build','--locked','--offline','--release','--lib','--manifest-path','rust/Cargo.toml',
         '--features','sunfish-native-startup-linux-rmtfs','--target','rust/targets/aarch64-sel4-microkit-pixel4k.json',
         '-Z','build-std=core,alloc','-Z','json-target-spec'],'sel4-library',native_env)
    if sha(FIRMWARE)!=FIRMWARE_HASH or any(sha(ROOT/p)!=h for p,h in hashes.items()):
        raise ValueError('Inputs changed during validation')
    report={'result':'passed','kernel_commit':PIN,'archive_sha256':ARCHIVE_HASH,'firmware_sha256':FIRMWARE_HASH,
            'source_sha256':hashes,'linux_source_sha256':original_hashes,'dtbs':dtbs,'groups':int(re.search(r'test result: ok\. (\d+) passed',regular.stdout)[1]),
            'mutations_rejected':list(mutants),'private_image_target_bytes_compared':0x9800000,
            'metadata_bytes_compared':9100,'original_c_metadata_sha256':sha(OUT/'metadata.bin'),
            'original_c_target_sha256':sha(OUT/'target.bin'),'original_c_pas_trace_sha256':sha(OUT/'pas-trace.txt'),
            'native_memory_and_loader_aarch64_object':sha(OUT/'native-aarch64.o'),'sel4_library_built':True,
            'differences':['three availability queries before INIT','bounded 4 KiB chunks with pre-AUTH readback',
                           'metadata and target retained until proven quiescent; no automatic cleanup/retry'],
            'scope':'Offline loader/native-memory comparison only; not composed with actual MPSS power, ready IRQ or native GLINK handoff.',
            'device_actions':[],'native_mpss_started':False,'native_wifi_proven':False}
    (OUT/'result.json').write_text(json.dumps(report,indent=2)+'\n')
    print(f'PASS: {report["groups"]} groups, complete private image vs original Linux C, seven rejected faults, AArch64 native memory/loader and seL4 library. No device action.')


if __name__=='__main__': main()
