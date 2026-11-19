#!/usr/bin/env python3
"""Selected native GLINK opening, retained-domain runtime and regressions."""
from pathlib import Path
import hashlib,importlib.util,json,os,re,shutil,subprocess,tomllib
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-linux-transport'
HARNESS=ROOT/'tests/sunfish_mpss_linux_transport_test.rs'
FEATURES=['sunfish-linux-ipa-qmi','sunfish-linux-rmtfs','sunfish-linux-mpss-auth','sunfish-linux-mpss-glink',
          'sunfish-native-startup-linux-rmtfs','sunfish-native-startup-mpss-smp2p','sunfish-native-startup-mpss-glink','sunfish-smmu-install-probe']
FLAGS=['--edition=2024','-D','warnings','-C','opt-level=2','--test','--cfg','mpss_glink_tests','--cfg','mpss_glink_transport_tests']
for f in FEATURES:FLAGS+=['--cfg',f'feature="{f}"']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,label,env=None,success=True):
    p=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(label+'.log')).write_text(p.stdout+p.stderr)
    if success and p.returncode:raise SystemExit(f'{label} failed; see {OUT/(label+".log")}')
    return p
def deps(p):
    for a,b in re.findall(r'#\[path\s*=\s*"([^"]+)"\]|include!\("([^"]+)"\)',p.read_text()):yield(p.parent/(a or b)).resolve()
def absolute(text,p):
    for name in re.findall(r'"([^"]+\.rs)"',text):
        q=(p.parent/name).resolve()
        if q.is_file():text=text.replace('"'+name+'"',json.dumps(str(q)))
    return text
def expand(p):return absolute(re.sub(r'include!\("([^"]+)"\);',lambda m:expand((p.parent/m[1]).resolve()),p.read_text()),p)
def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    if OUT.stat().st_mode&0o077:raise ValueError('private output required')
    paths={HARNESS,Path(__file__).resolve(),ROOT/'platforms/sunfish/audit-native-mpss-glink-transport.py',
           ROOT/'rust/Cargo.toml',ROOT/'rust/Cargo.lock',ROOT/'rust/src/lib.rs'};pending=[HARNESS]
    while pending:
        for p in deps(pending.pop()):
            if p not in paths:paths.add(p);pending.append(p)
    hashes={str(p.relative_to(ROOT)):sha(p)for p in sorted(paths)}
    for name in ['audit-native-mpss-glink-transport.py','test-native-mpss-linux-glink.py','test-native-mpss-postinstall.py']:
        run(['python3',str(ROOT/'platforms/sunfish'/name)],name)
    env=dict(os.environ,SUNFISH_LINUX_RMTFS_C=str(OUT.parent/'linux-rmtfs/linux-scm.txt'),
             SUNFISH_RMTFS_WIRE_C=str(OUT.parent/'linux-rmtfs-server/wire.txt'),
             SUNFISH_GLK_MEMORY_C=str(OUT.parent/'mpss-glink-memory-linux'),SUNFISH_GLK_TRANSPORT_C=str(OUT.parent/'mpss-glink-transport-linux'))
    rustc=shutil.which('rustc');run([rustc,*FLAGS,str(HARNESS),'-o',str(OUT/'test')],'host-build',env)
    tests=run([str(OUT/'test'),'--test-threads=1'],'host-tests',env)
    total=int(re.search(r'test result: ok\. (\d+) passed',tests.stdout)[1]);groups=len(re.findall(r'test .*linux_transport_.* \.\.\. ok',tests.stdout))
    if groups!=18:raise ValueError('all eighteen transport/domain groups required')
    run([rustc,'--edition=2024','-D','warnings','--test',str(ROOT/'tests/sunfish_glink_fifo_test.rs'),'-o',str(OUT/'legacy')],'feature-off-build',env)
    legacy=run([str(OUT/'legacy'),'--test-threads=1'],'feature-off-tests',env)
    legacy_count=int(re.search(r'test result: ok\. (\d+) passed',legacy.stdout)[1])
    protocol=ROOT/'rust/src/sunfish_mpss_linux_transport.rs';fifo=ROOT/'rust/src/qcom_glink_fifo.rs'
    native=ROOT/'rust/src/sunfish_mpss_linux_transport_native.rs';domain=ROOT/'rust/src/sunfish_smmu_native.rs'
    signals=ROOT/'rust/src/sunfish_mpss_linux_smp2p_native.rs'
    # Gate quarantine is behaviorally covered by the auth-root gate tests.
    # Mutating only this call is masked here by Retained's terminal `failed` latch.
    changes={
        'skip-local-open-ack':(protocol,'            && self.open_acked\n            && self.remote_acked','            && self.remote_acked'),
        'hardcode-peer-channel':(protocol,'self.channels.remote.ok_or(Error::Negotiation)?,','2,'),
        'reset-rx-at-handoff':(protocol,'self.ring.control = Some(Runtime {','self.ring.io.store_rx_tail(0).unwrap();self.ring.words[2]=0;self.ring.control = Some(Runtime {'),
        'drop-early-records':(protocol,'early: self.early,','early: [None;8],'),
        'accept-torn-record':(protocol,'if a != b || a[..8] != first {','if false {'),
        'ignore-peer-feature-mask':(protocol,'self.channels.features &= value;\n                    self.peer_version = true;','self.peer_version = true;'),
        'bypass-enclosing-domain':(domain,'self.with_retained_modem(|memory| memory.glink(action))','self.shared_memory.as_mut().ok_or(Error::State)?.glink(action).map_err(Error::SharedMemory)'),
        'notify-smp2p-instead':(signals,'self.notify_bit(1 << 12)','self.notify_bit(1 << 14)'),
        'wait-for-global-ready':(native,'ready: p != smp::GlinkPoll::Unstable,','ready: p != smp::GlinkPoll::Unstable && c.report().ready_seen,'),
        'allow-preauth-glink-doorbell':(signals,'o.lifecycle < Lifecycle::AuthReturned || o.power != Power::Active','o.power != Power::Active'),
    }
    for name,(target,before,after)in changes.items():
        source=target.read_text()
        if source.count(before)!=1:raise ValueError('mutation must be unique: '+name)
        folder=OUT/name;folder.mkdir(exist_ok=True)
        (folder/'module.rs').write_text(absolute(source.replace(before,after),target));harness=expand(HARNESS)
        if target==protocol:
            (folder/'fifo.rs').write_text(absolute(fifo.read_text(),fifo).replace(json.dumps(str(protocol)),json.dumps(str(folder/'module.rs'))))
            harness=harness.replace(json.dumps(str(fifo)),json.dumps(str(folder/'fifo.rs')))
        else:harness=harness.replace(json.dumps(str(target)),json.dumps(str(folder/'module.rs')))
        (folder/'test.rs').write_text(harness)
        # Removed checks can make one import, parameter or private forwarder
        # unused. Each rejection must be behavioral after successful compile.
        run([rustc,*FLAGS,'-A','unused_variables','-A','unused_imports','-A','dead_code',str(folder/'test.rs'),'-o',str(folder/'test')],name+'-build',env)
        if run([str(folder/'test'),'linux_transport_','--test-threads=1'],name,env,False).returncode==0:raise ValueError('accepted mutation '+name)
    package=OUT/'aarch64';package.mkdir(exist_ok=True)
    (package/'Cargo.toml').write_text('[package]\nname="sunfish-mpss-glink-transport-check"\nversion="0.0.0"\nedition="2024"\n[lib]\npath="native.rs"\n[dependencies]\npersonal-os-rust-server={path='+json.dumps(str(ROOT/'rust'))+',default-features=false,features=["sunfish-native-startup-mpss-glink"]}\n')
    shutil.copyfile(ROOT/'rust/Cargo.lock',package/'Cargo.lock')
    (package/'native.rs').write_text('''#![no_std]
use personal_os_rust_server::{qcom_glink_fifo as f,qcom_glink_qmi::Session,
 sunfish_mpss_linux_loader::AuthReturned,sunfish_smmu_native as d,sunfish_mpss_linux_transport_native as n};
type D<'a>=d::Native<'a,fn(usize)->Result<(),d::Error>>;
type I<'m>=n::Retained<'m,fn(usize)->Result<(),d::Error>>;
type G<'m>=personal_os_rust_server::sunfish_mpss_live_domain::NativeGate<D<'m>>;
type B<'m>=f::BootTransport<I<'m>>;
pub fn start<'m>(g:G<'m>,a:AuthReturned,s:Session,hz:u64)->B<'m>{n::start_live(g,a,s,hz)}
pub fn step(b:&mut B<'_>,now:u64)->Result<f::BootPhase,f::Error>{b.step(now)}
pub fn report(b:&B<'_>)->f::BootReport{b.report()}
pub fn attach<'m>(b:B<'m>)->Result<f::Adapter<I<'m>>,B<'m>>{b.into_adapter()}
pub fn receive(a:&mut f::Adapter<I<'_>>,g:u64)->Result<f::Receive,f::Error>{a.poll_receive(g)}
pub fn publish(a:&mut f::Adapter<I<'_>>,g:u64)->Result<f::Transmit,f::Error>{a.publish_next(g)}
pub fn control(a:&mut f::Adapter<I<'_>>,g:u64)->Result<(),f::Error>{a.start_control(g)}
''')
    sdk=ROOT/'.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish';target=OUT.parent/'preflight-target'
    native_env=dict(env,SEL4_INCLUDE_DIRS=str(sdk/'board/sunfish/debug/include'),CARGO_TARGET_DIR=str(target),
        SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(OUT.parent/'prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo','+nightly-2026-09-25','build','--offline','--release','--manifest-path',str(package/'Cargo.toml'),
         '--target',str(ROOT/'rust/targets/aarch64-sel4-microkit-pixel4k.json'),'-Z','build-std=core,alloc','-Z','json-target-spec'],'aarch64-concrete',native_env)
    original=tomllib.loads((ROOT/'rust/Cargo.lock').read_text())['package'];original={(p['name'],p['version'],p.get('source')):p for p in original}
    for p in tomllib.loads((package/'Cargo.lock').read_text())['package']:
        if p['name']!='sunfish-mpss-glink-transport-check' and original.get((p['name'],p['version'],p.get('source')))!=p:raise ValueError('changed pinned dependency '+p['name'])
    obj=target/'aarch64-sel4-microkit-pixel4k/release/libsunfish_mpss_glink_transport_check.rlib'
    for rel,h in hashes.items():
        p=ROOT/rel
        if sha(p)!=h:raise ValueError('changed input '+rel)
        q=OUT/'source'/rel;q.parent.mkdir(parents=True,exist_ok=True);q.write_bytes(p.read_bytes())
    proofs={str(p.relative_to(ROOT)):sha(p)for p in [OUT.parent/'mpss-glink-transport-linux/result.json',OUT.parent/'mpss-linux-glink/result.json',OUT.parent/'mpss-postinstall/result.json']}
    result={'result':'passed','source_sha256':hashes,'prerequisite_proofs':proofs,'transport_groups':groups,'total_host_groups':total,
        'feature_off_fifo_groups':legacy_count,'mutations_rejected':list(changes),'aarch64_concrete_rlib_sha256':sha(obj),
        'aarch64_concrete_wrapper_sha256':sha(package/'native.rs'),'pinned_dependencies_unchanged':True,
        'device_actions':[],'native_mpss_started':False,'native_wifi_proven':False,
        'scope':'Selected Linux VERSION/IPCRTR control bytes, both opening directions, dynamic RX, deferred intents and QRTR data over a retained installed native SMMU/SMEM/APCS owner in CPU RAM; concrete seL4 AArch64 instantiation.',
        'remaining':['concrete root reset-lifecycle and PAS4/power/capability bindings','real GLINK/SMP2P IRQ/deadline loop and original boot-service owners',
                     'cold native MPSS authentication/readiness and WLFW discovery','native WLAN startup, association and verified network traffic']}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'PASS: {groups} transport/domain groups, {total} total host groups, {legacy_count} feature-off FIFO groups, eleven rejected mutants, original Linux C and concrete seL4 AArch64. No device action.')
if __name__=='__main__':main()
