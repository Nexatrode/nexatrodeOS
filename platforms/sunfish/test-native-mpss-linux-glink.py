#!/usr/bin/env python3
"""Validate retained GLINK memory ownership offline, without device access."""
from pathlib import Path
import hashlib,json,os,re,shutil,subprocess,sys,tomllib
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-linux-glink'
HARNESS=ROOT/'tests/sunfish_mpss_linux_glink_test.rs'
FEATURES=['sunfish-linux-ipa-qmi','sunfish-linux-rmtfs','sunfish-linux-mpss-auth',
          'sunfish-linux-mpss-glink','sunfish-native-startup-linux-rmtfs','sunfish-native-startup-mpss-smp2p']
FLAGS=['--edition=2024','-D','warnings','-C','opt-level=2','--cfg','mpss_glink_tests','--test']
for feature in FEATURES:FLAGS+=['--cfg',f'feature="{feature}"']
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
def expand(p):
    text=re.sub(r'include!\("([^"]+)"\);',lambda m:expand((p.parent/m[1]).resolve()),p.read_text())
    return absolute(text,p)
def verify_reused_proof(p):
    """Reuse requires the exact source closure and referenced proof bytes."""
    d=json.loads(p.read_text())
    if d.get('result','passed')!='passed' or not d.get('source_sha256'):
        raise ValueError('missing successful proof '+str(p))
    for rel,h in d['source_sha256'].items():
        candidates=[ROOT/rel,p.parent/'linux-source'/rel,p.parent/'source'/rel]
        q=next((q for q in candidates if q.is_file()),None)
        if q is None or sha(q)!=h:raise ValueError('stale reused input '+rel)
    for rel,h in d.get('original_c_and_legacy_proofs',{}).items():
        q=Path(rel)
        if sha(q)!=h:raise ValueError('changed prerequisite '+rel)
        verify_reused_proof(q)
    for key,rel in [('linux_c_proof_sha256','mpss-smp2p-linux/result.json'),
                    ('upstream_manifest_sha256','linux-rmtfs/manifest.json')]:
        if key in d:
            q=OUT.parent/rel
            if sha(q)!=d[key]:raise ValueError('changed prerequisite '+str(q))
            verify_reused_proof(q)
    # These are the actual original-C output files consumed by this harness.
    for name in ['wire.txt','linux-scm.txt']:
        if name in d.get('artifacts',{}):
            if sha(p.parent/name)!=d['artifacts'][name]['sha256']:
                raise ValueError('changed original-C fixture '+name)
def main():
    os.umask(0o077);OUT.mkdir(mode=0o700,parents=True,exist_ok=True)
    if OUT.stat().st_mode&0o077:raise ValueError('private output required')
    paths={HARNESS,Path(__file__).resolve(),ROOT/'platforms/sunfish/audit-native-mpss-glink-memory.py',
           ROOT/'rust/Cargo.toml',ROOT/'rust/Cargo.lock',ROOT/'rust/src/lib.rs'}
    pending=[HARNESS]
    while pending:
        for p in deps(pending.pop()):
            if p not in paths:paths.add(p);pending.append(p)
    hashes={str(p.relative_to(ROOT)):sha(p)for p in sorted(paths)}
    # Refresh proofs for changed feature-on and feature-off composition.
    run(['python3',str(ROOT/'platforms/sunfish/audit-native-mpss-glink-memory.py')],'audit-native-mpss-glink-memory.py')
    for name,proof in [('test-native-mpss-linux-boot.py','mpss-linux-boot/result.json'),
                       ('test-native-mpss-linux-smp2p.py','mpss-linux-smp2p/result.json')]:
        if '--reuse-prerequisites' in sys.argv:verify_reused_proof(OUT.parent/proof)
        else:run(['python3',str(ROOT/'platforms/sunfish'/name)],name)
    env=dict(os.environ,SUNFISH_LINUX_RMTFS_C=str(OUT.parent/'linux-rmtfs/linux-scm.txt'),
             SUNFISH_RMTFS_WIRE_C=str(OUT.parent/'linux-rmtfs-server/wire.txt'),
             SUNFISH_GLK_MEMORY_C=str(OUT.parent/'mpss-glink-memory-linux'))
    rustc=shutil.which('rustc')
    run([rustc,*FLAGS,str(HARNESS),'-o',str(OUT/'test')],'host-build',env)
    tests=run([str(OUT/'test'),'--test-threads=1'],'host-tests',env)
    total=int(re.search(r'test result: ok\. (\d+) passed',tests.stdout)[1])
    groups=len(re.findall(r'test .*glink_memory_.* \.\.\. ok',tests.stdout))
    if groups!=11:raise ValueError('all eleven GLINK memory groups required')
    protocol=ROOT/'rust/src/sunfish_mpss_linux_glink.rs';native=ROOT/'rust/src/sunfish_mpss_linux_smem_native.rs'
    changes={
        'accept-preauth-lifecycle':(protocol,'if o.lifecycle < Lifecycle::AuthReturned || o.power != Power::Active {','if o.power != Power::Active {'),
        'accept-replaced-rx':(protocol,'if old.is_some() && old != d.item {','if false {'),
        'forget-ap-cursor-ownership':(protocol,'words[1] != r.tx_head\n            || words[2] != r.rx_tail','false\n            || false'),
        'assume-fixed-rx-length':(protocol,'report.rx = Some(item);','report.rx = Some(Item {size:16384,..item});'),
        'reuse-after-uncertain-store':(protocol,'Err(e) => Err(self.glink_poison(e)),','Err(e) => {self.busy=false;Err(e)},'),
        'native-write-peer-cursor':(native,'(1, ITEMS[2].1.size)','(0, ITEMS[2].1.size)'),
        'native-write-smp-item':(native,'let at = ITEMS[2].1.offset - FIRST_PAGE + r.offset();','let at = ITEMS[0].1.offset - FIRST_PAGE + r.offset();'),
        'native-open-cursor-whitelist':(native,'if r.offset() != word','if false'),
    }
    # Relocate the protocol's parent as well as its child so that tests retain
    # their exact visibility and still instantiate the real native adapter.
    parent=ROOT/'rust/src/sunfish_mpss_linux_smp2p.rs'
    for name,(target,before,after)in changes.items():
        text=target.read_text()
        if text.count(before)!=1:raise ValueError('mutation must be unique: '+name)
        folder=OUT/name;folder.mkdir(exist_ok=True)
        (folder/'module.rs').write_text(absolute(text.replace(before,after),target))
        harness=expand(HARNESS)
        if target==protocol:
            (folder/'parent.rs').write_text(absolute(parent.read_text(),parent).replace(json.dumps(str(protocol)),json.dumps(str(folder/'module.rs'))))
            harness=harness.replace(json.dumps(str(parent)),json.dumps(str(folder/'parent.rs')))
        else:harness=harness.replace(json.dumps(str(native)),json.dumps(str(folder/'module.rs')))
        (folder/'test.rs').write_text(harness)
        # Redundant checks intentionally become unused in individual mutants.
        # Rejection requires a compiled executable with a behavioral failure.
        run([rustc,*FLAGS,'-A','unused_variables','-A','dead_code',str(folder/'test.rs'),'-o',str(folder/'test')],name+'-build',env)
        if run([str(folder/'test'),'glink_memory_','--test-threads=1'],name,env,False).returncode==0:
            raise ValueError('accepted mutation '+name)
    package=OUT/'aarch64';package.mkdir(exist_ok=True)
    (package/'Cargo.toml').write_text('[package]\nname="sunfish-mpss-glink-memory-check"\nversion="0.0.0"\nedition="2024"\n[lib]\npath="native.rs"\n[dependencies]\npersonal-os-rust-server={path='+json.dumps(str(ROOT/'rust'))+',default-features=false,features=["sunfish-linux-mpss-glink","sunfish-native-startup-mpss-smp2p"]}\n')
    shutil.copyfile(ROOT/'rust/Cargo.lock',package/'Cargo.lock')
    (package/'native.rs').write_text('''#![no_std]
use personal_os_rust_server::{sunfish_ipa_smem_native::Window,
 sunfish_mpss_linux_smem_native::Native,sunfish_mpss_linux_smp2p_native::Platform,
 sunfish_mpss_linux_smp2p as g,sunfish_mpss_linux_loader::AuthReturned,sunfish_mpss_linux_smem::Error};
type Owner<'a>=g::Controller<Window<'a>,Native<'a,fn(u64)->Result<(),Error>>,Platform<'a>>;
pub fn claim(c:&mut Owner<'_>,r:AuthReturned)->Result<(),g::GlinkError>{c.claim_glink(r)}
pub fn poll(c:&mut Owner<'_>)->Result<g::GlinkPoll,g::GlinkError>{c.glink_poll()}
pub fn descriptor(c:&mut Owner<'_>)->Result<[u32;8],g::GlinkError>{c.glink_descriptor()}
pub fn read_tx(c:&mut Owner<'_>,at:usize,out:&mut[u8])->Result<(),g::GlinkError>{c.glink_read_tx(at,out)}
pub fn read_rx(c:&mut Owner<'_>,at:usize,out:&mut[u8])->Result<(),g::GlinkError>{c.glink_read_rx(at,out)}
pub fn write(c:&mut Owner<'_>,at:usize,bytes:&[u8])->Result<(),g::GlinkError>{c.glink_write_tx(at,bytes)}
pub fn tx_head(c:&mut Owner<'_>,v:u32)->Result<(),g::GlinkError>{c.glink_store_tx_head(v)}
pub fn rx_tail(c:&mut Owner<'_>,v:u32)->Result<(),g::GlinkError>{c.glink_store_rx_tail(v)}
''')
    sdk=ROOT/'.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    target=OUT.parent/'preflight-target'
    native_env=dict(env,SEL4_INCLUDE_DIRS=str(sdk/'board/sunfish/debug/include'),CARGO_TARGET_DIR=str(target),
        SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(OUT.parent/'prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo','+nightly-2026-09-25','build','--offline','--release','--manifest-path',str(package/'Cargo.toml'),
        '--target',str(ROOT/'rust/targets/aarch64-sel4-microkit-pixel4k.json'),'-Z','build-std=core,alloc','-Z','json-target-spec'],'aarch64-concrete',native_env)
    original=tomllib.loads((ROOT/'rust/Cargo.lock').read_text())['package']
    original={(p['name'],p['version'],p.get('source')):p for p in original}
    for p in tomllib.loads((package/'Cargo.lock').read_text())['package']:
        key=p['name'],p['version'],p.get('source')
        if p['name']!='sunfish-mpss-glink-memory-check' and original.get(key)!=p:
            raise ValueError('concrete build changed pinned dependency '+p['name'])
    obj=target/'aarch64-sel4-microkit-pixel4k/release/libsunfish_mpss_glink_memory_check.rlib'
    for rel,h in hashes.items():
        p=ROOT/rel
        if sha(p)!=h:raise ValueError('input changed '+rel)
        dest=OUT/'source'/rel;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(p.read_bytes())
    proofs={str(p.relative_to(ROOT)):sha(p)for p in [OUT.parent/'mpss-glink-memory-linux/result.json',
        OUT.parent/'mpss-linux-boot/result.json',OUT.parent/'mpss-linux-smp2p/result.json']}
    result={'result':'passed','source_sha256':hashes,'prerequisite_proofs':proofs,
        'glink_memory_groups':groups,'total_host_groups':total,'mutations_rejected':list(changes),
        'aarch64_concrete_rlib_sha256':sha(obj),'aarch64_concrete_wrapper_sha256':sha(package/'native.rs'),
        'pinned_dependencies_unchanged':True,'device_actions':[],'native_mpss_started':False,'native_wifi_proven':False,
        'scope':'Actual loader receipt and retained original SMEM/volatile adapters in CPU RAM, plus concrete no_std seL4 AArch64 memory methods. Host fixture emulates only TCSR claim semantics and injected failures.',
        'remaining':['root reset lifecycle, PAS4 bindings and reserved memory capabilities','retained native GLINK frontend and VERSION/OPEN/intent negotiation',
                     'IRQ/deadline dispatch and QRTR boot-service publication during MPSS ready wait','cold native MPSS/WLFW startup and network traffic proof']}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'PASS: {groups} GLINK memory groups, {total} total host groups, eight rejected mutations, original Linux C and concrete seL4 AArch64. No device action.')
if __name__=='__main__':main()
