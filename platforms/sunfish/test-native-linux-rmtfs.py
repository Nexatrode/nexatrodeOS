#!/usr/bin/env python3
"""Offline selected-Linux RMTFS ownership/retained RAM resource comparison."""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs'
PIN = '2267e248204e7e051e0f7355298949a6440de579'
ARCHIVE_HASH = 'd55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(args, label, env=None, success=True):
    r = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)
    (OUT / (label + '.log')).write_text(r.stdout + r.stderr)
    if success and r.returncode:
        raise SystemExit(f'{label}: exit {r.returncode}; see {OUT / (label + ".log")}')
    return r


def block(text, start):
    at = text.index(start)
    opening = text.index('{', at)
    end, depth = opening + 1, 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    if text[end:end + 1] == ';':
        end += 1
    return text[at:end] + '\n'


def oracle():
    archive = ROOT / '.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive) != ARCHIVE_HASH:
        raise ValueError('Wrong pinned Linux archive')
    names = ['drivers/soc/qcom/rmtfs_mem.c', 'drivers/firmware/qcom/qcom_scm.c',
             'drivers/firmware/qcom/qcom_scm.h', 'include/linux/firmware/qcom/qcom_scm.h',
             'include/dt-bindings/firmware/qcom,scm.h',
             'arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts']
    originals = {}
    with tarfile.open(archive) as t:
        members = {m.name.split('/', 1)[1]: m for m in t if '/' in m.name}
        for name in names:
            data = t.extractfile(members[name]).read()
            p = OUT / 'linux-source' / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
            originals[name] = data.decode()
    dts = originals[names[-1]]
    node = block(dts, 'rmtfs_mem: rmtfs@fde01000 {')
    assert 'reg = <0x0 0xfde01000 0x0 0x200000>;' in node
    assert 'qcom,client-id = <1>;' in node and 'qcom,vmid = <QCOM_SCM_VMID_MSS_MSA>;' in node
    assert 'no-map;' in node and 'qcom,use-guard-pages' not in node
    header = originals[names[2]]
    scm = originals[names[1]]
    driver = originals[names[0]]
    driver_body = driver[driver.index('perms[0].vmid = QCOM_SCM_VMID_HLOS;'):]
    driver_body = driver_body[:driver_body.index('if (ret < 0)')]
    vmids = originals[names[4]]
    perms = originals[names[3]]
    c = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <assert.h>
#include <errno.h>
typedef uint32_t u32;typedef uint64_t u64;typedef u32 __le32;typedef u64 __le64;
typedef u64 phys_addr_t;
#define cpu_to_le32(x) ((u32)(x))
#define cpu_to_le64(x) ((u64)(x))
#define __free(x)
#define GFP_KERNEL 0
#define SZ_64 64
#define ALIGN(x,n) (((x)+(n)-1)&~((n)-1))
#define BIT(n) (1ULL<<(n))
#define BITS_PER_TYPE(t) (sizeof(t)*8)
#define hweight64(x) __builtin_popcountll(x)
#define EXPORT_SYMBOL_GPL(x)
#define dev_err(...) ((void)0)
#define ARM_SMCCC_OWNER_SIP 2
'''
    c += header
    for text, definitions in [(vmids, ['QCOM_SCM_VMID_HLOS', 'QCOM_SCM_VMID_MSS_MSA']),
                              (perms, ['QCOM_SCM_PERM_READ', 'QCOM_SCM_PERM_WRITE', 'QCOM_SCM_PERM_RW'])]:
        for name in definitions:
            c += re.search(r'^#define ' + name + r'\s+[^\n]+', text, re.M)[0] + '\n'
    c += block(scm, 'struct qcom_scm_current_perm_info {')
    c += block(scm, 'struct qcom_scm_mem_map_info {')
    c += block(perms, 'struct qcom_scm_vmperm {')
    c += r'''
static unsigned char memory[320];
static struct {void *dev,*mempool;} scm_stub;
static __typeof__(scm_stub) *__scm=&scm_stub;
static struct qcom_scm_desc sent;
static int transport_status, command_status;
static void *qcom_tzmem_alloc(void *p,size_t n,int f){(void)p;(void)f;assert(n==192);memset(memory,0,sizeof(memory));return memory;}
static phys_addr_t qcom_tzmem_to_phys(void *p){return 0x81000000ULL+(unsigned char*)p-memory;}
static int qcom_scm_call(void *d,const struct qcom_scm_desc *desc,struct qcom_scm_res *res){(void)d;sent=*desc;res->result[0]=command_status;return transport_status;}
'''
    c += block(scm, 'static int __qcom_scm_assign_mem(')
    c += block(scm, 'int qcom_scm_assign_mem(')
    c += r'''
static int selected_rmtfs_probe(void){
 struct {u64 addr,size,perms;} r={.addr=0xfde01000,.size=0x200000},*rmtfs_mem=&r;
 struct qcom_scm_vmperm perms[3];u32 vmid[]={QCOM_SCM_VMID_MSS_MSA};int num_vmids=1,i,ret;
'''
    c += driver_body
    c += r'''
 if(ret==0)assert(r.perms==(BIT(3)|BIT(15)));
 return ret;
}
'''
    package = ROOT / '.cache/sunfish-mobian-comparison/wifi-preparation/source/rmtfs_1.1.orig.tar.gz'
    if sha(package) != '3d9d37cd573a44acf9c10897b59a3f9a359c0bcf2f02d9e2b4605f6cdf4ed5c5':
        raise ValueError('Wrong reference userspace source archive')
    with tarfile.open(package) as t:
        for name in ['sharedmem.c', 'rmtfs.c']:
            member = next(m for m in t if m.name.endswith('/' + name))
            data = t.extractfile(member).read()
            dest = OUT / 'linux-source/userspace-rmtfs-1.1' / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
    shared = (OUT / 'linux-source/userspace-rmtfs-1.1/sharedmem.c').read_text()
    # Linux LP64 uses unsigned long for these 64-bit fields; Darwin's uint64_t
    # spelling differs and would otherwise warn on the original %lx log.
    c += 'struct rmtfs_mem {unsigned long address,size;};\n'
    c += block(shared, 'int64_t rmtfs_mem_alloc(')
    c += r'''
int main(void){
 (void)qcom_scm_remap_error(-1);
 _Static_assert(sizeof(struct rmtfs_mem)==16,"LP64 reference fields");
 assert(sizeof(struct qcom_scm_current_perm_info)==24);
 assert(selected_rmtfs_probe()==0);
 assert(sent.svc==12&&sent.cmd==22&&sent.owner==2&&sent.arginfo==0x1117);
 assert(sent.args[0]==0x81000040&&sent.args[1]==16&&sent.args[2]==0x81000000);
 for(unsigned i=3;i<10;i++)memcpy(memory+256+8*(i-3),&sent.args[i],8);
 for(unsigned i=0;i<320;i++)printf("%02x",memory[i]);puts("");
 transport_status=-5;assert(selected_rmtfs_probe()==-5);
 transport_status=0;command_status=-7;assert(selected_rmtfs_probe()==-7);
 struct rmtfs_mem r={0xfde01000,0x200000};
 for(unsigned n=1;n<=0x200000;n*=2)assert(rmtfs_mem_alloc(&r,n)==0xfde01000);
 assert(rmtfs_mem_alloc(&r,0x200001)==-EINVAL);
 /* Native schema deliberately rejects the source helper's zero-size case. */
 assert(rmtfs_mem_alloc(&r,0)==0xfde01000);
 return 0;
}
'''
    (OUT / 'linux-scm.c').write_text(c)
    run(['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-Wno-sign-compare', str(OUT / 'linux-scm.c'), '-o', str(OUT / 'linux-scm')], 'linux-scm-build')
    (OUT / 'linux-scm.txt').write_text(run([str(OUT / 'linux-scm')], 'linux-scm-exec').stdout)
    # Preserve only non-secret service inventories from the successful run.
    evidence = ROOT / '.cache/sunfish-mobian-comparison/device-runs/cold-wifi-20261025T200752Z/cold-wifi-final-metadata.tar.gz'
    with tarfile.open(evidence) as t:
        for name in ['providers-before-modem.txt', 'providers-after-start.txt']:
            data = t.extractfile('./logs/' + name).read()
            (OUT / name).write_bytes(data)
            rows = [list(map(int, line.split()[:5])) for line in data.decode().splitlines()[1:]]
            assert not any(row[0] == 0x34 for row in rows)
            local = {tuple(row[:3]) for row in rows if row[3] == 1}
            assert {(14, 1, 0), (4096, 1, 0), (49, 1, 1)} <= local
    names += ['userspace-rmtfs-1.1/sharedmem.c', 'userspace-rmtfs-1.1/rmtfs.c']
    return {name: sha(OUT / 'linux-source' / name) for name in names}, sha(evidence)


def paths(text):
    return re.sub(r'#\[path\s*=\s*"([^"]+)"\]',
                  lambda m: '#[path="' + str((ROOT / 'tests' / m[1]).resolve()) + '"]', text)


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    source_hashes, evidence_hash = oracle()
    env = dict(os.environ, SUNFISH_LINUX_RMTFS_C=str(OUT / 'linux-scm.txt'))
    rustc = shutil.which('rustc')
    flags = [rustc, '--edition=2024', '-D', 'warnings']
    test = ROOT / 'tests/sunfish_linux_rmtfs_test.rs'
    binary = OUT / 'test'
    run(flags + ['--test', str(test), '-o', str(binary)], 'rust-build', env)
    r = run([str(binary), 'linuxrmtfs'], 'resource-tests')
    groups = int(re.search(r'test result: ok\. (\d+) passed;', r.stdout)[1])
    regression = run([str(binary)], 'all-tests')
    all_groups = int(re.search(r'test result: ok\. (\d+) passed;', regression.stdout)[1])
    fifo = (ROOT / 'tests/sunfish_glink_fifo_test.rs').read_text()
    declarations = paths(fifo.split('use qcom_glink_fifo::', 1)[0])
    names = ['qcom_modem_handlers', 'qcom_modem_storage', 'qcom_modem_ram_storage',
             'sunfish_lpd', 'sunfish_linux_rmtfs', 'sunfish_linux_rmtfs_native']
    wrapper = OUT / 'native.rs'
    wrapper.write_text('#![no_std]\n' + declarations + '\n' + '\n'.join(
        f'#[path="{ROOT}/rust/src/{name}.rs"]pub mod {name};' for name in names) + r'''
pub struct Gate(pub fn()->Result<sunfish_linux_rmtfs::Observation,qcom_modem_handlers::Fault>);
impl sunfish_linux_rmtfs_native::Gate for Gate {fn observe(&mut self)->Result<sunfish_linux_rmtfs::Observation,qcom_modem_handlers::Fault>{(self.0)()}}
pub struct Smc(pub fn(&sunfish_linux_rmtfs::Call)->Result<[u64;8],qcom_modem_handlers::Fault>);
impl sunfish_linux_rmtfs_native::Smc for Smc {fn invoke(&mut self,c:&sunfish_linux_rmtfs::Call)->Result<[u64;8],qcom_modem_handlers::Fault>{(self.0)(c)}}
pub type Resources=sunfish_linux_rmtfs::Resources<'static,sunfish_linux_rmtfs_native::Native<Gate,Smc>>;
pub fn prepare(r:&mut Resources)->Result<bool,qcom_modem_handlers::Fault>{r.prepare_step()}
pub fn share(r:&mut Resources)->Result<(),qcom_modem_handlers::Fault>{r.share()}
pub fn transfer(r:&mut Resources,p:qcom_modem_services::Partition,s:u32,a:u64,n:u32,w:bool)->Result<(),qcom_modem_handlers::Fault>{r.transfer(p,s,a,n,w)?;r.sync(p)}
''')
    run(flags + ['--crate-type=lib', '--target=aarch64-unknown-none', str(wrapper), '-o', str(OUT / 'native-aarch64.rlib')], 'aarch64-native', env)
    # Mutations are behaviorally rejected, not merely compiler failures.
    parent = ROOT / 'rust/src/sunfish_linux_rmtfs.rs'
    original = parent.read_text()
    flat = test.read_text().replace('include!("sunfish_linux_rmtfs_fixture.rs");',
        (ROOT / 'tests/sunfish_linux_rmtfs_fixture.rs').read_text())
    flat = flat.replace('include!("sunfish_modem_owner_test.rs");',
        (ROOT / 'tests/sunfish_modem_owner_test.rs').read_text().replace('include!("sunfish_glink_fifo_test.rs");', fifo))
    flat = paths(flat)
    mutants = {
        'ignore-command-status': ('if reply[0] != 0 || reply[1] != 0 {', 'if reply[0] != 0 {'),
        'accept-old-pool': ('a.physical_address != BUFFER_PA', 'false'),
        'missing-cold-source': ('&& o.initial_hlos', ''),
        'no-attempt-consumption': ('self.status.assignments_attempted = 1;', 'self.status.assignments_attempted = 0;'),
        'overflow-buffer-end': ('e > u64::from(BUFFER_BYTES)', 'false && e > u64::from(BUFFER_BYTES)'),
        'reverse-storage-direction': ('if write {', 'if !write {'),
        'corrupt-copy-to-modem': ('p.write(physical, &buffer[..bytes])?;', 'p.write(physical, &[0; IO_BYTES][..bytes])?;'),
    }
    for label, (before, after) in mutants.items():
        if original.count(before) != 1:
            raise ValueError('Mutation not unique: ' + label)
        folder = OUT / label
        folder.mkdir(exist_ok=True)
        (folder / parent.name).write_text(original.replace(before, after))
        source = folder / 'test.rs'
        source.write_text(flat.replace(str(parent), str(folder / parent.name)))
        exe = folder / 'test'
        run(flags + ['--test', str(source), '-o', str(exe)], label + '-build', env)
        if run([str(exe), 'linuxrmtfs'], label, success=False).returncode == 0:
            raise ValueError('Mutation accepted: ' + label)
    sdk = ROOT / '.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    native_env = dict(env, SEL4_INCLUDE_DIRS=str(sdk / 'board/sunfish/debug/include'),
                      CARGO_TARGET_DIR=str(ROOT / '.cache/sunfish-native-wifi/private/preflight-target'),
                      SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(ROOT / '.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo', '+nightly-2026-09-25', 'build', '--locked', '--offline', '--release', '--lib',
         '--manifest-path', 'rust/Cargo.toml', '--features', 'sunfish-native-startup-ipa-receive,sunfish-linux-ipa-qmi',
         '--target', 'rust/targets/aarch64-sel4-microkit-pixel4k.json', '-Z', 'build-std=core,alloc', '-Z', 'json-target-spec'], 'sel4-library', native_env)
    selected = [str(test.relative_to(ROOT)), 'tests/sunfish_linux_rmtfs_fixture.rs', 'tests/sunfish_modem_owner_test.rs', 'tests/sunfish_glink_fifo_test.rs',
                'platforms/sunfish/test-native-linux-rmtfs.py', 'rust/src/lib.rs', 'rust/Cargo.toml', 'rust/Cargo.lock',
                'docs/sunfish-native-wifi-startup.md']
    selected += ['rust/src/' + name + '.rs' for name in names]
    selected += [str((ROOT / 'tests' / path).resolve().relative_to(ROOT)) for path in re.findall(r'#\[path\s*=\s*"([^"]+)"\]', fifo.split('use qcom_glink_fifo::', 1)[0])]
    selected += ['rust/src/qcom_modem_owner.rs', 'tests/modem_handlers_fixture.rs']
    selected = sorted(set(selected))
    for name in selected:
        dest = OUT / 'source' / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, dest)
    manifest = {'linux_commit': PIN, 'linux_archive_sha256': ARCHIVE_HASH,
                'linux_source_sha256': source_hashes, 'cold_reference_evidence_sha256': evidence_hash,
                'behavioral_groups': groups, 'legacy_regression_groups': all_groups - groups,
                'source_sha256': {name: sha(ROOT / name) for name in selected},
                'mutations_rejected': list(mutants), 'aarch64_native_resources_monomorphized': True,
                'sel4_library_built': True, 'device_actions': [],
                'scope': 'One selected RMTFS reservation and SCM byte parity, bounded guarded volatile copies with real verified RAM shadows. No heap dependency established by the reference inventory. Native capability/mapping factory, QRTR storage server hookup, MPSS startup and Wi-Fi still unproven.'}
    artifacts = [p for p in OUT.iterdir() if p.is_file() and p.name != 'manifest.json'
                 and not p.name.startswith('skip-copy-to-modem')]
    artifacts += [p for label in mutants for p in (OUT / label).iterdir() if p.is_file()]
    manifest['artifacts'] = {str(p.relative_to(OUT)): {'bytes': p.stat().st_size, 'sha256': sha(p)} for p in artifacts}
    (OUT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'PASS: {groups} RMTFS resource groups, {all_groups-groups} regressions, seven rejected mutations, original Linux SCM oracle, AArch64 native resources and seL4 library; no device action.')


if __name__ == '__main__':
    main()
