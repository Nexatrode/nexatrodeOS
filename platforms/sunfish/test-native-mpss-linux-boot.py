#!/usr/bin/env python3
"""Validate the retained PAS4/RMTFS boundary offline; no phone operations."""
from pathlib import Path
import hashlib,json,os,re,shutil,subprocess,tomllib
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-linux-boot'
FLAGS=['--edition=2024','-D','warnings','-C','opt-level=2','--test',
       '--cfg','feature="sunfish-linux-ipa-qmi"','--cfg','feature="sunfish-linux-rmtfs"',
       '--cfg','feature="sunfish-linux-mpss-auth"']
HARNESS=ROOT/'tests/sunfish_mpss_linux_boot_test.rs'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,label,env=None,success=True):
    p=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(label+'.log')).write_text(p.stdout+p.stderr)
    if success and p.returncode:raise SystemExit(f'{label} failed; see {OUT/(label+".log")}')
    return p
def deps(p):
    for a,b in re.findall(r'#\[path\s*=\s*"([^"]+)"\]|include!\("([^"]+)"\)',p.read_text()):
        yield (p.parent/(a or b)).resolve()
def absolute(text,p):
    for name in re.findall(r'"([^"]+\.rs)"',text):
        q=(p.parent/name).resolve()
        if q.is_file():text=text.replace('"'+name+'"',json.dumps(str(q)))
    return text
def main():
    os.umask(0o077);OUT.mkdir(mode=0o700,parents=True,exist_ok=True)
    if OUT.stat().st_mode&0o077:raise ValueError('private output required')
    paths={HARNESS,Path(__file__).resolve(),ROOT/'rust/src/lib.rs',ROOT/'rust/Cargo.toml',ROOT/'rust/Cargo.lock'}
    pending=[HARNESS]
    while pending:
        for p in deps(pending.pop()):
            if p not in paths:paths.add(p);pending.append(p)
    hashes={str(p.relative_to(ROOT)):sha(p)for p in sorted(paths)}
    # Refresh the exact original-C comparisons and feature-off regressions for
    # the primitives changed by this opt-in composition, before using them.
    for name in ['test-native-mpss-linux-loader.py','test-native-linux-rmtfs-server.py']:
        run(['python3',str(ROOT/'platforms/sunfish'/name)],name)
    env=dict(os.environ,SUNFISH_LINUX_RMTFS_C=str(OUT.parent/'linux-rmtfs/linux-scm.txt'),
             SUNFISH_RMTFS_WIRE_C=str(OUT.parent/'linux-rmtfs-server/wire.txt'))
    rustc=shutil.which('rustc');run([rustc,*FLAGS,str(HARNESS),'-o',str(OUT/'test')],'host-build',env)
    tests=run([str(OUT/'test'),'--test-threads=1'],'host-tests',env)
    total=int(re.search(r'test result: ok\. (\d+) passed',tests.stdout)[1])
    groups=len(re.findall(r'test .*boot_handoff::.* \.\.\. ok',tests.stdout))
    if groups!=10:raise ValueError('all ten handoff groups required')
    boot=ROOT/'rust/src/sunfish_mpss_linux_boot.rs';native=ROOT/'rust/src/sunfish_linux_rmtfs_native.rs'
    pool=ROOT/'rust/src/sunfish_linux_rmtfs.rs'
    changes={
        'skip-actual-pool-transition':(boot,'self.server\n                .submit_mpss_auth(self.generation, r)\n                .map_err(|_| pas::Error::Guard)?;',''),
        'native-accepts-incomplete-assignment':(native,'self.auth_attempted || !self.assignment_completed','self.auth_attempted'),
        'native-replays-reset-marker':(native,'self.auth_attempted || !self.assignment_completed','!self.assignment_completed'),
        'pool-accepts-stopped-runtime':(pool,'|| o.modem_stopped {','|| false {'),
        'pool-forgets-cold-preauth-check':(pool,'fn pre_auth_shared(&mut self, generation: u64) -> Result<(), Fault> {\n        self.check(true)?;',
            'fn pre_auth_shared(&mut self, generation: u64) -> Result<(), Fault> {\n        self.check(false)?;'),
        'expose-server-before-auth':(boot,'if self.report().phase != Phase::AuthReturned || self.server.runtime_shared().is_err() {',
            'if self.server.validate().is_err() {'),
    }
    for name,(target,before,after) in changes.items():
        text=target.read_text()
        if text.count(before)!=1:raise ValueError('mutation must be unique: '+name)
        folder=OUT/name;folder.mkdir(exist_ok=True)
        (folder/'module.rs').write_text(absolute(text.replace(before,after),target))
        harness=absolute(HARNESS.read_text(),HARNESS)
        # Included root harnesses must be expanded so their #[path] declarations
        # can select this mutant while preserving their original directory.
        def expand(p):
            text=p.read_text()
            text=re.sub(r'include!\("([^"]+)"\);',lambda m:expand((p.parent/m[1]).resolve()),text)
            return absolute(text,p)
        harness=expand(HARNESS).replace(json.dumps(str(target)),json.dumps(str(folder/'module.rs')))
        (folder/'test.rs').write_text(harness)
        # Removing a call deliberately makes its private forwarding chain
        # unused. Keep all other diagnostics strict and require a behavioral
        # failure, rather than counting a dead-code warning as rejection.
        run([rustc,*FLAGS,'-A','dead_code',str(folder/'test.rs'),'-o',str(folder/'test')],name+'-build',env)
        if run([str(folder/'test'),'auth_handoff_','--test-threads=1'],name,env,False).returncode==0:
            raise ValueError('accepted mutation '+name)
    # Compile concrete no_std Boot<Window,Native<Gate,Monitor>> entry points
    # for the actual seL4 AArch64 target. Callbacks are placeholders for the
    # future root bindings, not hardware authority or native startup evidence.
    package=OUT/'aarch64';package.mkdir(exist_ok=True)
    (package/'Cargo.toml').write_text('[package]\nname="sunfish-mpss-auth-boundary-check"\nversion="0.0.0"\nedition="2024"\n[lib]\npath="native.rs"\n[dependencies]\npersonal-os-rust-server={path='+json.dumps(str(ROOT/'rust'))+',default-features=false,features=["sunfish-linux-mpss-auth"]}\n')
    shutil.copyfile(ROOT/'rust/Cargo.lock',package/'Cargo.lock')
    (package/'native.rs').write_text('''#![no_std]
use personal_os_rust_server::{qcom_modem_handlers::Fault,sunfish_linux_rmtfs as r,
 sunfish_linux_rmtfs_native as n,sunfish_mpss_linux_loader as p,
 sunfish_mpss_linux_memory::Window,sunfish_mpss_linux_boot as b};
pub struct Gate{pub observe:fn()->Result<r::Observation,Fault>,pub submit:fn(u64,&p::Request)->Result<(),Fault>}
impl n::Gate for Gate{fn observe(&mut self)->Result<r::Observation,Fault>{(self.observe)()}}
impl n::AuthGate for Gate{fn submit_mpss_auth(&mut self,g:u64,r:&p::Request)->Result<(),Fault>{(self.submit)(g,r)}}
pub struct Monitor(pub fn(&r::Call)->Result<[u64;8],Fault>);
impl n::Smc for Monitor{fn invoke(&mut self,r:&r::Call)->Result<[u64;8],Fault>{(self.0)(r)}}
pub struct Backend{pub guard:fn(p::Guard)->Result<(),p::Error>,pub acquire:fn()->Result<u64,p::Error>,pub invoke:fn(&p::Request)->Result<[u64;8],p::Error>,pub release:fn(u64)->Result<(),p::Error>}
impl p::Backend for Backend{type Lease=u64;fn guard(&mut self,g:p::Guard)->Result<(),p::Error>{(self.guard)(g)}fn acquire(&mut self)->Result<u64,p::Error>{(self.acquire)()}fn invoke(&mut self,r:&p::Request)->Result<[u64;8],p::Error>{(self.invoke)(r)}fn release(&mut self,l:u64)->Result<(),p::Error>{(self.release)(l)}}
pub fn step(owner:&mut b::Boot<'_,'_,Window<'_>,n::Native<Gate,Monitor>>,backend:&mut Backend)->Result<p::Phase,b::Error>{owner.step(backend)}
pub fn server(owner:&mut b::Boot<'_,'_,Window<'_>,n::Native<Gate,Monitor>>)->Result<(),b::Error>{let _=owner.server()?;Ok(())}
''')
    sdk=ROOT/'.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    target=ROOT/'.cache/sunfish-native-wifi/private/preflight-target'
    native_env=dict(env,SEL4_INCLUDE_DIRS=str(sdk/'board/sunfish/debug/include'),CARGO_TARGET_DIR=str(target),
        SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(OUT.parent/'prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo','+nightly-2026-09-25','build','--offline','--release','--manifest-path',str(package/'Cargo.toml'),
        '--target',str(ROOT/'rust/targets/aarch64-sel4-microkit-pixel4k.json'),'-Z','build-std=core,alloc','-Z','json-target-spec'],'aarch64-concrete',native_env)
    original=tomllib.loads((ROOT/'rust/Cargo.lock').read_text())['package']
    original={(p['name'],p['version'],p.get('source')):p for p in original}
    for p in tomllib.loads((package/'Cargo.lock').read_text())['package']:
        key=p['name'],p['version'],p.get('source')
        if p['name']!='sunfish-mpss-auth-boundary-check' and original.get(key)!=p:
            raise ValueError('concrete build changed pinned dependency '+p['name'])
    obj=target/'aarch64-sel4-microkit-pixel4k/release/libsunfish_mpss_auth_boundary_check.rlib'
    for rel,h in hashes.items():
        p=ROOT/rel
        if sha(p)!=h:raise ValueError('input changed '+rel)
        dest=OUT/'source'/rel;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(p.read_bytes())
    proofs={str(p):sha(p)for p in [OUT.parent/'mpss-linux-loader/result.json',OUT.parent/'linux-rmtfs-server/manifest.json']}
    result={'result':'passed','source_sha256':hashes,'original_c_and_legacy_proofs':proofs,
        'handoff_groups':groups,'total_host_groups':total,'mutations_rejected':list(changes),
        'aarch64_concrete_rlib_sha256':sha(obj),'aarch64_concrete_wrapper_sha256':sha(package/'native.rs'),
        'pinned_dependencies_unchanged':True,'device_actions':[],'native_mpss_started':False,'native_wifi_proven':False,
        'scope':'Actual PAS4 loader, native reserved-pool adapter, original RAM-shadow server and QRTR storage dispatch in host RAM; concrete no_std AArch64 composition compiled.',
        'remaining':['root reset-lifecycle and actual PAS4 monitor/power bindings','reserved modem/metadata capabilities and private input integration',
                     'native runtime GLINK/QRTR/IRQ dispatch during ready wait','cold native MPSS/WLFW startup and network traffic proof']}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'PASS: {groups} boot-handoff groups, {total} total host groups, six rejected mutations, refreshed original-C/legacy proofs and concrete AArch64 build. No device action.')
if __name__=='__main__':main()
