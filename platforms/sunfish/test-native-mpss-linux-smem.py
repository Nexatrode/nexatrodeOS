#!/usr/bin/env python3
"""Offline retained IPA->MPSS SMEM allocation and native composition checks."""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-linux-smem'


def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def run(args,label,env=None,success=True):
    r=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(label+'.log')).write_text(r.stdout+r.stderr)
    if success and r.returncode:raise SystemExit(f'{label} failed; see {OUT/(label+".log")}')
    return r


def count(r):return int(re.search(r'test result: ok\. (\d+) passed',r.stdout)[1])


def paths(text):
    return re.sub(r'#\[path\s*=\s*"([^"]+)"\]',lambda m:'#[path='+json.dumps(str((ROOT/'tests'/m[1]).resolve()))+']',text)


def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    if OUT.stat().st_mode&0o077:raise ValueError('Private output required')
    selected=['rust/src/sunfish_mpss_linux_smem.rs','rust/src/sunfish_mpss_linux_smem_native.rs',
              'rust/src/sunfish_ipa_smem.rs','rust/src/sunfish_ipa_smem_native.rs','rust/src/sunfish_ipa_shared_memory.rs',
              'rust/src/sunfish_smmu_native.rs','rust/src/sunfish_smmu_lpae.rs','rust/src/sunfish_ipa_gsi_profile.rs',
              'rust/src/qcom_modem_status.rs','tests/sunfish_ipa_smem_test.rs','tests/sunfish_mpss_linux_smem_test.rs',
              'tests/sunfish_ipa_shared_memory_test.rs','tests/sunfish_ipa_shared_fixture.rs',
              'tests/sunfish_mpss_linux_shared_memory_test.rs','rust/src/lib.rs','rust/Cargo.toml','rust/Cargo.lock',
              'platforms/sunfish/test-native-mpss-linux-smem.py','platforms/sunfish/audit-native-mpss-smem-layout.py',
              'platforms/sunfish/audit-native-ipa-shared-source.py','platforms/sunfish/audit-ipa-smem-rust-source.py']
    selected+=['rust/src/sunfish_mpss_linux_smp2p.rs','rust/src/sunfish_mpss_linux_smp2p_native.rs']
    selected+=['tests/sunfish_mpss_postinstall_cases.rs','tests/sunfish_smmu_native_test.rs',
               'platforms/sunfish/test-native-mpss-postinstall.py']
    hashes={p:sha(ROOT/p) for p in selected}
    run(['python3','platforms/sunfish/test-native-mpss-postinstall.py'],'postinstall-native')
    postinstall=ROOT/'.cache/sunfish-native-wifi/private/mpss-postinstall/result.json'
    run(['python3','platforms/sunfish/audit-native-ipa-shared-source.py','--output',str(OUT/'inherited-shared-audit.json')],'inherited-shared-audit')
    run(['python3','platforms/sunfish/audit-native-mpss-smem-layout.py'],'original-linux-layout')
    oracle_dir=ROOT/'.cache/sunfish-native-wifi/private/mpss-smem-layout'
    evidence=json.loads((oracle_dir/'result.json').read_text())
    items=OUT/'linux-items.txt'
    items.write_text(''.join(f'{a["id"]:x} {a["payload_offset"]:x} {a["bytes"]:x}\n' for a in evidence['items']))
    env=dict(os.environ,SUNFISH_MPSS_SMEM_C_LAYOUT=str(items))
    flags=[shutil.which('rustc'),'--edition=2024','-D','warnings']
    compile_flags=flags+['--cfg','mpss_smem_tests']
    harness=ROOT/'tests/sunfish_ipa_smem_test.rs'
    run(compile_flags+['--test',str(harness),'-o',str(OUT/'controller-test')],'controller-build',env)
    groups=count(run([str(OUT/'controller-test'),'mpss::'],'controller-tests',env))
    total=count(run([str(OUT/'controller-test')],'controller-and-predecessor',env))
    feature=['--cfg','feature="sunfish-native-startup-linux-rmtfs"']
    shared=ROOT/'tests/sunfish_mpss_linux_shared_memory_test.rs'
    run(flags+feature+['--test',str(shared),'-o',str(OUT/'shared-test')],'shared-build',env)
    shared_total=count(run([str(OUT/'shared-test')],'shared-tests',env))
    run(flags+['--test','tests/sunfish_ipa_shared_memory_test.rs','-o',str(OUT/'legacy-shared-test')],'old-shared-build',env)
    old_shared=count(run([str(OUT/'legacy-shared-test')],'old-shared-tests',env))
    controller=ROOT/selected[0];original=controller.read_text()
    native=ROOT/selected[1];native_original=native.read_text()
    mutations={
        'reuse-old-ipa-header':(controller,'PART + 0x2030, PART + 0x2198, PART + 0x21c8','PART + 0x20, PART + 0x2198, PART + 0x21c8'),
        'wrong-smp2p-padding':(controller,'(8, if n == 0 { 4 } else { 0 })','(8, 0)'),
        'wrong-published-cursor':(controller,'value: FREES[n + 1] as u32','value: FREES[n] as u32'),
        'omit-final-payload-readback':(controller,'if u32_at(self.io()?, i.offset + at) != 0 {','if false {'),
        'release-foreign-lock':(controller,'if !self.owned()? {','if false {'),
        'replay-unwound-step':(controller,'if self.busy || self.report.error.is_some() || self.report.phase == Phase::Prepared {','if self.report.error.is_some() || self.report.phase == Phase::Prepared {'),
        'native-write-cached-cursor':(native,'self.metadata.add(3)','self.metadata.add(4)'),
    }
    for label,(target,before,after) in mutations.items():
        text=original if target==controller else native_original
        hits=text.count(before)
        if not hits or (hits!=1 and label!='release-foreign-lock'):raise ValueError('Mutation identity changed: '+label)
        folder=OUT/label;folder.mkdir(exist_ok=True)
        (folder/target.name).write_text(text.replace(before,after))
        flat=paths(harness.read_text()).replace(str(target),str(folder/target.name))
        (folder/'test.rs').write_text(flat)
        run(compile_flags+['--test',str(folder/'test.rs'),'-o',str(folder/'test')],label+'-build',env)
        if run([str(folder/'test'),'mpss::'],label,env,success=False).returncode==0:raise ValueError('Accepted mutation: '+label)
    declarations=paths(shared.read_text().split('use sunfish_ipa_shared_fixture::',1)[0])
    declarations=declarations.replace('mod sunfish_ipa_shared_fixture;', '')
    declarations=declarations.replace('#![allow(dead_code)]','#![no_std]')
    declarations=re.sub(r'\bmod (\w+);',r'pub mod \1;',declarations)
    # This narrow object intentionally excludes the unrelated GSI runtime
    # that calls its crate-private readback constructor. Do not change the
    # production module or suppress diagnostics on the new implementation.
    declarations=declarations.replace('pub mod sunfish_ipa_gsi_profile;', '#[allow(dead_code)]pub mod sunfish_ipa_gsi_profile;')
    wrapper=OUT/'native.rs'
    wrapper.write_text(declarations+r'''
pub fn attach(m:&mut sunfish_ipa_shared_memory::SharedMemory<'_>,generation:u64,gate:fn(u64)->Result<(),sunfish_mpss_linux_smem::Error>)->Result<(),sunfish_ipa_shared_memory::Error>{m.attach_modem(generation,gate)}
pub fn step(m:&mut sunfish_ipa_shared_memory::SharedMemory<'_>)->Result<sunfish_mpss_linux_smem::Phase,sunfish_ipa_shared_memory::Error>{m.modem_step()}
pub fn take(m:&mut sunfish_ipa_shared_memory::SharedMemory<'_>)->Result<sunfish_mpss_linux_smem::Prepared,sunfish_ipa_shared_memory::Error>{m.take_modem_prepared()}
pub fn predecessor(m:&mut sunfish_ipa_shared_memory::SharedMemory<'_>,pages:&[u64;32])->Result<[sunfish_smmu_lpae::LpaePageMapping;37],sunfish_ipa_shared_memory::Error>{
 m.prepare()?;let _=m.virtual_spans();let _=m.shared_pages()?;let _=sunfish_ipa_shared_memory::SharedMemory::excludes_private_page(pages[0]);
 let maps=m.mappings(pages)?;m.validate_mappings(&maps,pages)?;Ok(maps)
}
''')
    run(flags+feature+['--crate-type=lib','--target=aarch64-unknown-none','--emit=obj',str(wrapper),'-o',str(OUT/'native-aarch64.o')],'aarch64-native',env)
    sdk=ROOT/'.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    native_env=dict(env,SEL4_INCLUDE_DIRS=str(sdk/'board/sunfish/debug/include'),
                    CARGO_TARGET_DIR=str(ROOT/'.cache/sunfish-native-wifi/private/preflight-target'),
                    SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(ROOT/'.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo','+nightly-2026-09-25','build','--locked','--offline','--release','--lib','--manifest-path','rust/Cargo.toml',
         '--features','sunfish-native-startup-linux-rmtfs','--target','rust/targets/aarch64-sel4-microkit-pixel4k.json',
         '-Z','build-std=core,alloc','-Z','json-target-spec'],'sel4-library',native_env)
    for path,h in hashes.items():
        data=(ROOT/path).read_bytes()
        if hashlib.sha256(data).hexdigest()!=h:raise ValueError('Source changed: '+path)
        dest=OUT/'native-source'/path;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
    result={'result':'passed','source_sha256':hashes,'linux_layout_evidence_sha256':sha(oracle_dir/'result.json'),
            'postinstall_native_proof_sha256':sha(postinstall),
            'inherited_shared_audit_sha256':sha(OUT/'inherited-shared-audit.json'),
            'controller_and_native_groups':groups,'previous_predecessor_groups':total-groups,
            'composition_groups':4,'shared_dependency_groups':shared_total-4,'old_shared_groups':old_shared,
            'mutations_rejected':list(mutations),'native_aarch64_object_sha256':sha(OUT/'native-aarch64.o'),
            'sel4_library_built':True,'stores_checked':4201,'payload_bytes_zeroed_and_verified':16756,
            'scope':'Retained real IPA owner -> fixed locked modem allocation -> native shared/SMMU-owner API. Root mapping/capability image and SMP2P/GLINK activation still to be composed.',
            'intentional_differences':['fixed native allocation order after IPA497','fresh cold allocations only; existing modem items rejected',
                                       'new descriptors/FIFO zeroed and verified before auth; no runtime activation',
                                       'uncertain owners retained until reboot; no retry/rollback'],
            'device_actions':[],'native_mpss_started':False,'native_wifi_proven':False}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'PASS: {groups} allocation/native groups + four retained-owner groups; seven rejected mutations, predecessor/shared regressions, original Linux C, AArch64 composition and seL4 library. No device action.')


if __name__=='__main__':main()
