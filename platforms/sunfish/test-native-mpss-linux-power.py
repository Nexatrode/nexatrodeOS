#!/usr/bin/env python3
"""Offline MPSS power + original SCM lease regressions + concrete native MMIO."""
from pathlib import Path
import hashlib,json,os,re,shutil,subprocess
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-linux-power'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,label,env=None,success=True):
    r=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True);(OUT/(label+'.log')).write_text(r.stdout+r.stderr)
    if success and r.returncode:raise SystemExit(f'{label} failed: {OUT/(label+".log")}')
    return r
def count(r):return int(re.search(r'test result: ok\. (\d+) passed',r.stdout)[1])
def paths(s,base):return re.sub(r'#\[path\s*=\s*"([^"]+)"\]',lambda m:'#[path='+json.dumps(str((base/m[1]).resolve()))+']',s)
def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    if OUT.stat().st_mode&0o077:raise ValueError('private output required')
    names=['qcom_bcm','qcom_bcm_vote','qcom_cmd_db','qcom_mdt','qcom_pas_init','qcom_pas_metadata','qcom_rpmh_bus','qcom_rpmh_handoff','qcom_rpmh_read',
           'sunfish_ipa_fws','sunfish_ipa_linux_pas','sunfish_ipa_pas','sunfish_scm_linux_power','qcom_qmp','qcom_qmp_link','sunfish_mpss_linux_power','sunfish_mpss_linux_power_native']
    selected=['rust/src/'+n+'.rs'for n in names]+['rust/src/pixel_rpmh_bus.rs','rust/src/pixel_rpmh_controller.rs','rust/src/pixel_rpmh_handoff.rs',
              'tests/sunfish_scm_linux_power_test.rs','tests/sunfish_mpss_linux_power_test.rs','rust/src/lib.rs','rust/Cargo.toml','rust/Cargo.lock',
              'platforms/sunfish/test-native-mpss-linux-power.py','platforms/sunfish/audit-native-mpss-power.py','platforms/sunfish/audit-native-mpss-smem-layout.py']
    hashes={p:sha(ROOT/p)for p in selected}
    run(['python3','platforms/sunfish/audit-native-mpss-power.py'],'original-linux-c')
    oracle=ROOT/'.cache/sunfish-native-wifi/private/mpss-power-linux'
    env=dict(os.environ,SUNFISH_MPSS_POWER_C_LEVELS=str(oracle/'levels.txt'),SUNFISH_MPSS_POWER_C_PACKET=str(oracle/'on-packet.bin'))
    rust=[shutil.which('rustc'),'--edition=2024','-D','warnings'];harness=ROOT/'tests/sunfish_scm_linux_power_test.rs'
    flags=rust+['--cfg','mpss_power_tests','--test']
    run(flags+[str(harness),'-o',str(OUT/'tests')],'host-build',env)
    total=count(run([str(OUT/'tests'),'--test-threads=1'],'host-tests',env))
    groups=count(run([str(OUT/'tests'),'mpss::','--test-threads=1'],'modem-tests',env))
    power=ROOT/'rust/src/sunfish_mpss_linux_power.rs';native=ROOT/'rust/src/sunfish_mpss_linux_power_native.rs';scm=ROOT/'rust/src/sunfish_scm_linux_power.rs'
    mutations={
        'fixed-selector-instead-of-db':(power,'(count - 1) as u32','6u32'),
        'regulator-consumer-offset':(power,'(count - 1) as u32','count as u32'),
        'child-before-parent':(power,'ReadResource::Mx,\n    ReadResource::Cx,','ReadResource::Cx,\n    ReadResource::Mx,'),
        'omit-parent-vote':(power,'self.report.last_rail = Some(rail.resource);','if n == 0 { continue; } self.report.last_rail = Some(rail.resource);'),
        'downstream-96-byte-length':(native,'write_volatile(64u32.to_le())','write_volatile(96u32.to_le())'),
        'notify-wrong-peer-bit':(native,'write_volatile(1u32.to_le())','write_volatile((1u32 << 14).to_le())'),
        'replay-unwound-load-state':(power,'if self.busy || self.used || self.report.error.is_some() {','if self.report.error.is_some() {'),
        'replace-outstanding-secure-lease':(scm,'if self.failure.get().is_some() || self.outstanding.get().is_some() {','if self.failure.get().is_some() {'),
    }
    for label,(target,before,after)in mutations.items():
        original=target.read_text()
        if original.count(before)!=1:raise ValueError('changed mutation identity: '+label)
        folder=OUT/label;folder.mkdir(exist_ok=True);(folder/target.name).write_text(original.replace(before,after))
        root=paths(harness.read_text(),harness.parent).replace(str(target),str(folder/target.name));(folder/'test.rs').write_text(root)
        run(flags+[str(folder/'test.rs'),'-o',str(folder/'test')],label+'-build',env)
        if run([str(folder/'test'),'mpss::','--test-threads=1'],label,env,False).returncode==0:raise ValueError('accepted mutation: '+label)
    wrapper=OUT/'native.rs'
    # The narrow object omits consumers of legacy fixed commands/PAS queries.
    # Suppress only those old modules' dead-code lint, never the new code's.
    wrapper.write_text('#![no_std]\nextern crate self as personal_os_rust_server;\n'+'\n'.join(('#[allow(dead_code)]' if n in ['qcom_pas_init','qcom_rpmh_read'] else '')+f'#[path="{ROOT/"rust/src"/(n+".rs")}"]pub mod {n};'for n in names)+
                      f'\n#[path="{ROOT/"rust/src/pixel_rpmh_bus.rs"}"]pub mod native_rsc;\n'+r'''
pub struct G{pub check:fn(u64,bool)->Result<(),sunfish_mpss_linux_power::Error>,pub wait:fn()->Result<(),sunfish_mpss_linux_power::Error>}
impl sunfish_mpss_linux_power_native::Gate for G{fn check(&mut self,g:u64,p:bool)->Result<(),sunfish_mpss_linux_power::Error>{(self.check)(g,p)}fn wait(&mut self)->Result<(),sunfish_mpss_linux_power::Error>{(self.wait)()}}
type Hold<'a>=sunfish_mpss_linux_power::HoldOnce<sunfish_mpss_linux_power_native::Native<'a,G>>;
pub fn enable(h:&mut Hold<'_>,io:&mut native_rsc::Controller)->Result<(),sunfish_mpss_linux_power::Error>{h.enable(io,65536,core::hint::spin_loop,|_|{})}
pub fn verify(h:&mut Hold<'_>,io:&mut native_rsc::Controller)->Result<(),sunfish_mpss_linux_power::Error>{h.verify(io,65536,core::hint::spin_loop)}
pub fn lease(h:&mut Hold<'_>,scm:&sunfish_scm_linux_power::HoldOnce,io:&mut native_rsc::Controller)->Result<(),u64>{
 use sunfish_ipa_linux_pas::Resources;let mut r=sunfish_mpss_linux_power::Resources::new(h,scm,io,65536).map_err(sunfish_mpss_linux_power::Error::code)?;let l=r.acquire()?;r.release(l)
}
''')
    run(rust+['--crate-type=lib','--target=aarch64-unknown-none','--emit=obj',str(wrapper),'-o',str(OUT/'native-aarch64.o')],'aarch64-native',env)
    sdk=ROOT/'.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    native_env=dict(env,SEL4_INCLUDE_DIRS=str(sdk/'board/sunfish/debug/include'),CARGO_TARGET_DIR=str(ROOT/'.cache/sunfish-native-wifi/private/preflight-target'),
                    SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(ROOT/'.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo','+nightly-2026-09-25','build','--locked','--offline','--release','--lib','--manifest-path','rust/Cargo.toml','--features','sunfish-native-startup-mpss-smp2p',
         '--target','rust/targets/aarch64-sel4-microkit-pixel4k.json','-Z','build-std=core,alloc','-Z','json-target-spec'],'sel4-library',native_env)
    for path,digest in hashes.items():
        if sha(ROOT/path)!=digest:raise ValueError('source changed: '+path)
        p=OUT/'source'/path;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes((ROOT/path).read_bytes())
    result={'result':'passed','source_sha256':hashes,'modem_power_groups':groups,'inherited_scm_qmp_and_dependencies':total-groups,
            'original_linux_c_proof_sha256':sha(oracle/'result.json'),'mutations_rejected':list(mutations),
            'native_aarch64_object_sha256':sha(OUT/'native-aarch64.o'),'sel4_library_built':True,
            'scope':'Offline live-DB-selected modem power, concrete QMP/RSC and composed secure-call resource checks. No root entry/capability image, physical rail/notification proof, PAS4 start or Wi-Fi.',
            'device_actions':[],'native_mpss_started':False,'native_wifi_proven':False}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(f'PASS: {groups} modem-power/native groups, {total-groups} inherited groups, original Linux C, eight rejected mutations, concrete AArch64 and seL4 library. No device action.')
if __name__=='__main__':main()
