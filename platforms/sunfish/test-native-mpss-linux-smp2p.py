#!/usr/bin/env python3
"""Offline Linux wire comparison, retained native ownership and failure checks."""
from pathlib import Path
import hashlib,json,os,re,shutil,subprocess
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-linux-smp2p'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,label,env=None,success=True):
    r=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(label+'.log')).write_text(r.stdout+r.stderr)
    if success and r.returncode:raise SystemExit(f'{label} failed: {OUT/(label+".log")}')
    return r
def count(r):return int(re.search(r'test result: ok\. (\d+) passed',r.stdout)[1])
def paths(s,base):
    return re.sub(r'#\[path\s*=\s*"([^"]+)"\]',lambda m:'#[path='+json.dumps(str((base/m[1]).resolve()))+']',s)
def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    if OUT.stat().st_mode&0o077:raise ValueError('private output required')
    selected=['rust/src/sunfish_mpss_linux_smp2p.rs','rust/src/sunfish_mpss_linux_smp2p_native.rs',
              'rust/src/sunfish_mpss_linux_smem.rs','rust/src/sunfish_mpss_linux_smem_native.rs',
              'rust/src/sunfish_ipa_smem.rs','rust/src/sunfish_ipa_smem_native.rs','rust/src/qcom_modem_status.rs',
              'rust/src/sunfish_ipa_shared_memory.rs','rust/src/sunfish_smmu_native.rs','rust/src/sunfish_smmu_lpae.rs',
              'rust/src/sunfish_ipa_gsi_profile.rs','rust/src/lib.rs','rust/Cargo.toml','rust/Cargo.lock',
              'tests/sunfish_ipa_smem_test.rs','tests/sunfish_mpss_linux_smp2p_test.rs',
              'tests/sunfish_mpss_linux_smp2p_scope.rs','tests/sunfish_mpss_linux_shared_memory_test.rs',
              'tests/sunfish_ipa_shared_fixture.rs','platforms/sunfish/audit-native-mpss-smp2p.py',
              'platforms/sunfish/audit-native-mpss-smem-layout.py','platforms/sunfish/test-native-mpss-linux-smp2p.py']
    hashes={p:sha(ROOT/p)for p in selected}
    run(['python3','platforms/sunfish/audit-native-mpss-smp2p.py'],'original-linux-c')
    oracle=ROOT/'.cache/sunfish-native-wifi/private/mpss-smp2p-linux'
    env=dict(os.environ,SUNFISH_MPSS_SMP2P_ORACLE=str(oracle))
    rust=[shutil.which('rustc'),'--edition=2024','-D','warnings']
    harness=ROOT/'tests/sunfish_ipa_smem_test.rs';flags=rust+['--cfg','mpss_smp2p_tests','--test']
    run(flags+[str(harness),'-o',str(OUT/'controller')],'controller-build',env)
    total=count(run([str(OUT/'controller'),'--test-threads=1'],'controller-tests',env))
    features=['--cfg','feature="sunfish-native-startup-linux-rmtfs"','--cfg','feature="sunfish-native-startup-mpss-smp2p"']
    shared=ROOT/'tests/sunfish_mpss_linux_shared_memory_test.rs'
    run(rust+features+['--test',str(shared),'-o',str(OUT/'shared')],'shared-build',env)
    shared_total=count(run([str(OUT/'shared'),'--test-threads=1'],'shared-tests',env))
    protocol=ROOT/selected[0];native=ROOT/selected[3]
    mutations={
        'ignore-peer-fatal':(protocol,'if p.slave_kernel.is_some_and(|v| v & 1 != 0) {','if false {'),
        'accept-renamed-peer':(protocol,'p.names[..old.valid as usize] != old.names[..old.valid as usize]','false'),
        'publish-valid-before-enabled':(protocol,'self.store(56, enabled, Field::IpaEnabled, false)?;','self.store(56, enabled | 1, Field::IpaEnabled, false)?;'),
        'accept-torn-snapshot':(protocol,'if a == b {','if true {'),
        'replay-unwound-operation':(protocol,'if self.busy || self.report.error.is_some() || self.report.phase != Phase::Initialize {','if self.report.error.is_some() || self.report.phase != Phase::Initialize {'),
        'native-open-write-whitelist':(native,'if !valid || r.at() % 4 != 0','if false && !valid || r.at() % 4 != 0'),
        'native-write-next-word':(native,'let p = unsafe { self.pages.add(at / 4) };','let p = unsafe { self.pages.add(at / 4 + 1) };'),
        'notify-glink-bit-instead':(protocol,'pub const DOORBELL: u32 = 1 << 14;','pub const DOORBELL: u32 = 1 << 12;'),
    }
    for label,(target,before,after)in mutations.items():
        original=target.read_text()
        if original.count(before)!=1:raise ValueError('changed mutation identity: '+label)
        folder=OUT/label;folder.mkdir(exist_ok=True)
        (folder/target.name).write_text(paths(original.replace(before,after),target.parent))
        root=paths(harness.read_text(),harness.parent).replace(str(target),str(folder/target.name))
        (folder/'test.rs').write_text(root)
        run(flags+[str(folder/'test.rs'),'-o',str(folder/'test')],label+'-build',env)
        if run([str(folder/'test'),'--test-threads=1'],label,env,False).returncode==0:raise ValueError('accepted mutation '+label)
    # Compile concrete volatile adapters, shared handoff and runtime methods
    # for AArch64, rather than just exporting generic uninstantiated code.
    declarations=paths(shared.read_text().split('use sunfish_ipa_shared_fixture::',1)[0],shared.parent)
    declarations=declarations.replace('mod sunfish_ipa_shared_fixture;','').replace('#![allow(dead_code)]','#![no_std]')
    declarations=re.sub(r'\bmod (\w+);',r'pub mod \1;',declarations)
    declarations=declarations.replace('pub mod sunfish_ipa_gsi_profile;','#[allow(dead_code)]pub mod sunfish_ipa_gsi_profile;')
    wrapper=OUT/'native.rs';wrapper.write_text(declarations+r'''
pub fn activate<'a>(m:&mut sunfish_ipa_shared_memory::SharedMemory<'a>,r:sunfish_mpss_linux_smem::Prepared,s:sunfish_mpss_linux_smp2p_native::Platform<'a>)->Result<(),sunfish_ipa_shared_memory::Error>{m.activate_smp2p(r,s)}
pub fn poll(m:&mut sunfish_ipa_shared_memory::SharedMemory<'_>)->Result<sunfish_mpss_linux_smp2p::Poll,sunfish_ipa_shared_memory::Error>{m.smp2p_poll()}
pub fn ready(m:&mut sunfish_ipa_shared_memory::SharedMemory<'_>)->Result<sunfish_mpss_linux_smp2p::ReadyObservation,sunfish_ipa_shared_memory::Error>{m.take_mpss_ready_observation()}
pub fn predecessor(m:&mut sunfish_ipa_shared_memory::SharedMemory<'_>,g:u64,gate:fn(u64)->Result<(),sunfish_mpss_linux_smem::Error>,pages:&[u64;32])->Result<sunfish_mpss_linux_smem::Prepared,sunfish_ipa_shared_memory::Error>{
 m.prepare()?;let _=m.virtual_spans();let _=m.shared_pages()?;let _=sunfish_ipa_shared_memory::SharedMemory::excludes_private_page(pages[0]);
 let maps=m.mappings(pages)?;m.validate_mappings(&maps,pages)?;m.attach_modem(g,gate)?;let _=m.modem_step()?;m.take_modem_prepared()
}
''')
    run(rust+features+['--crate-type=lib','--target=aarch64-unknown-none','--emit=obj',str(wrapper),'-o',str(OUT/'native-aarch64.o')],'aarch64-native',env)
    sdk=ROOT/'.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    native_env=dict(env,SEL4_INCLUDE_DIRS=str(sdk/'board/sunfish/debug/include'),
                    CARGO_TARGET_DIR=str(ROOT/'.cache/sunfish-native-wifi/private/preflight-target'),
                    SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(ROOT/'.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo','+nightly-2026-09-25','build','--locked','--offline','--release','--lib','--manifest-path','rust/Cargo.toml',
         '--features','sunfish-native-startup-mpss-smp2p','--target','rust/targets/aarch64-sel4-microkit-pixel4k.json',
         '-Z','build-std=core,alloc','-Z','json-target-spec'],'sel4-library',native_env)
    for path,digest in hashes.items():
        if sha(ROOT/path)!=digest:raise ValueError('source changed: '+path)
        p=OUT/'source'/path;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes((ROOT/path).read_bytes())
    result={'result':'passed','source_sha256':hashes,'linux_c_proof_sha256':sha(oracle/'result.json'),
            'controller_and_native_groups':total-12,'previous_allocation_groups':12,
            'shared_owner_activation_groups':2,'previous_shared_and_dependency_groups':shared_total-2,
            'mutations_rejected':list(mutations),'native_aarch64_object_sha256':sha(OUT/'native-aarch64.o'),
            'sel4_library_built':True,'scope':'Offline selected SMP2P wire/native-memory/retained-owner handoff. No physical IRQ, PAS4 start, root image, native GLINK serviceability or Wi-Fi proof.',
            'intentional_differences':['fresh cold items only; no reuse/recovery','terminal uncertain operation retained until reboot',
                'fatal/stop/reset/name replacement rejected before further writes','bounded stable snapshots, not Linux nested IRQ scheduling'],
            'device_actions':[],'native_mpss_started':False,'native_wifi_proven':False}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'PASS: {total-12} SMP2P/native groups, two shared-owner activation groups, {total+shared_total-(total-12)-2} inherited groups, original Linux C, eight rejected mutations, AArch64 and seL4 library. No device action.')
if __name__=='__main__':main()
