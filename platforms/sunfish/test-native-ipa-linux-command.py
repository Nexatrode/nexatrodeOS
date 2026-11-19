#!/usr/bin/env python3
"""Offline command-channel START against the pinned full gsi_channel_start()."""
from pathlib import Path
import hashlib,importlib.util,json,os,re,shutil,subprocess,tarfile

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-command'
HERE=Path(__file__).resolve().parent
def module(name,file):
    spec=importlib.util.spec_from_file_location(name,HERE/file);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,name,env=None,expected=0):
    r=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(name+'.log')).write_text(r.stdout+r.stderr)
    if r.returncode!=expected:raise SystemExit(f'{name}: exit {r.returncode}; see {OUT/(name+".log")}')
    return r
def main():
    OUT.mkdir(mode=0o700,parents=True,exist_ok=True)
    ep=module('command_endpoint_reference','test-native-ipa-linux-endpoint.py')
    path=ep.OUT/'manifest.json'
    if not path.exists() or any(sha(ROOT/n)!=h for n,h in json.loads(path.read_text())['source_sha256'].items()):ep.main()
    g=module('command_gsi_reference','test-native-ipa-linux-gsi.py')
    assert sha(g.ARCHIVE)==g.ARCHIVE_HASH
    source={}
    with tarfile.open(g.ARCHIVE) as t:
        prefix=t.getnames()[0].split('/')[0]+'/'
        for rel in g.INPUTS:
            b=t.extractfile(prefix+rel).read();p=OUT/'linux-source'/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b);source[rel]=b.decode()
    c=g.c_oracle(source)
    # Extend the fixture's software IRQ state and command-state emulation;
    # the extracted Linux start/interrupt functions themselves are unchanged.
    replacements={
        'u32 channel_count; u32 evt_ring_count; u32 modem_channel_bitmap; int mutex;':'u32 channel_count; u32 evt_ring_count; u32 modem_channel_bitmap; int mutex; u32 ieob_enabled_bitmap;',
        'if (value >> 24) abort(); /* ALLOCATE only */':'if ((value >> 24) && (offset != 0x1f008 || value != 0x01000001)) abort();',
        '= 1 << 20;':'= (value >> 24 ? 2 : 1) << 20;',
    }
    for before,after in replacements.items():
        assert c.count(before)==1,before;c=c.replace(before,after)
    names=['gsi_irq_ieob_enable_one','gsi_irq_ieob_disable','gsi_irq_ieob_disable_one',
        'gsi_channel_start_command','__gsi_channel_start','gsi_channel_start']
    functions='static void napi_enable(struct napi_struct *n) { (void)n; }\nstatic void napi_disable(struct napi_struct *n) { (void)n; }\n'
    functions+='\n'.join(g.function(source[g.IPA+'gsi.c'],n) for n in names)
    c=c.replace('int main(void) {',functions+'\nint main(void) {',1)
    before,tail=c.rsplit('}',1)
    c=before+'''    setup_result = 3;
    if (gsi_channel_start(&g, 1)) abort();
    if (registers[0x1f088 / 4] != 0x4c || registers[0x1f0b8 / 4] != 1 ||
        ((registers[0x1c080 / 4] >> 20) & 15) != 2) abort();
}'''+tail
    cp=OUT/'linux-command-oracle.c';cp.write_text(c)
    cc=shutil.which('clang') or shutil.which('cc');assert cc
    run([cc,'-std=gnu11','-Wall','-Wextra','-Werror',str(cp),'-o',str(OUT/'linux-command-oracle')],'c-build')
    rows=run([str(OUT/'linux-command-oracle')],'c-oracle').stdout.splitlines()
    rows=[' '.join(r.split()[2:]) for r in rows if r.startswith('setup 3 ')]
    assert len(rows)==9
    golden=OUT/'linux-command-writes.txt';golden.write_text('\n'.join(rows)+'\n')
    # Reuse the already checked endpoint/GSI fixtures, binding their core
    # types to this test crate. Only cmdstart_ groups execute in this suite.
    text=(ROOT/'tests/sunfish_ipa_linux_endpoint_test.rs').read_text()
    text='use crate::{sunfish_ipa_linux_gsi,sunfish_ipa_linux_endpoint};\n'+text[text.index('mod fixture'):]
    text+='\npub fn owned_startup() -> (gsi::SetupOnce,ConfigureOnce) { let setup=fixture::completed_setup(); let mut endpoints=ConfigureOnce::new(); endpoints.execute(&setup,&mut Fake::new()).unwrap();(setup,endpoints) }\n'
    text+='pub fn incomplete_setup() -> gsi::SetupOnce {fixture::incomplete_setup()}\n'
    fixture=OUT/'endpoint-fixture.rs';fixture.write_text(text)
    env=dict(os.environ,SUNFISH_COMMAND_ENDPOINT_FIXTURE=str(fixture),SUNFISH_ENDPOINT_GSI_FIXTURE=str(ep.OUT/'gsi-fixture.rs'),SUNFISH_COMMAND_C_REFERENCE=str(golden))
    rustc=shutil.which('rustc');assert rustc
    binary=OUT/'command-test'
    run([rustc,'--edition=2024','-D','warnings','--test',str(ROOT/'tests/sunfish_ipa_linux_command_test.rs'),'-o',str(binary)],'rust-build',env)
    tested=run([str(binary),'cmdstart_'],'rust-tests',env)
    groups=int(re.search(r'test result: ok\. (\d+) passed;',tested.stdout)[1])
    wrapper=OUT/'core.rs';names=['sunfish_ipa_linux_buffers','sunfish_ipa_linux_gsi','sunfish_ipa_linux_endpoint','sunfish_ipa_linux_command']
    wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names))
    run([rustc,'--edition=2024','-D','warnings','--crate-type=lib','--target=aarch64-unknown-none',str(wrapper),'-o',str(OUT/'command-aarch64.rlib')],'aarch64-core')
    original=(ROOT/'rust/src/sunfish_ipa_linux_command.rs').read_text()
    harness=(ROOT/'tests/sunfish_ipa_linux_command_test.rs').read_text()
    mutations={'wrong-start-channel':('START: u32 = 0x01000001','START: u32 = 0x01000002'),
        'missing-ieob-type':('gsi::BASE_IRQ_TYPES | 8','gsi::BASE_IRQ_TYPES'),
        'wrong-ieob-event':('self.write(io, Register::IeobMask, 1)?','self.write(io, Register::IeobMask, 2)?')}
    for name,(before,after) in mutations.items():
        assert original.count(before)==1,before
        mutant=OUT/(name+'.rs');mutant.write_text(original.replace(before,after));body=harness
        for n in ['sunfish_ipa_linux_buffers','sunfish_ipa_linux_gsi','sunfish_ipa_linux_endpoint']:
            body=body.replace('../rust/src/'+n+'.rs',str(ROOT/'rust/src'/(n+'.rs')))
        body=body.replace('../rust/src/sunfish_ipa_linux_command.rs',str(mutant));test=OUT/(name+'-test.rs');test.write_text(body)
        executable=OUT/(name+'-test')
        run([rustc,'--edition=2024','-D','warnings','--test','--crate-name','command_mutation',str(test),'-o',str(executable)],name+'-build',env)
        run([str(executable),'--exact','cmdstart_matches_full_compiled_linux_start_including_ieob_and_irq_clear'],name,env,expected=101)
    selected=['rust/src/sunfish_ipa_linux_command.rs','tests/sunfish_ipa_linux_command_test.rs','platforms/sunfish/test-native-ipa-linux-command.py','rust/src/lib.rs','platforms/sunfish/test-modem-qmi.py']
    for rel in selected:
        p=OUT/'source'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/rel,p)
    manifest={'kernel_commit':g.PIN,'kernel_archive_sha256':g.ARCHIVE_HASH,'source_sha256':{n:sha(ROOT/n) for n in selected},
        'reference_sha256':{n:sha(OUT/'linux-source'/n) for n in source},'endpoint_prerequisite_manifest_sha256':sha(path),
        'compiled_c_writes':9,'encoder_mutations_rejected':len(mutations),'behavioral_groups':groups,'aarch64_no_std':True,'hardware_commands':[],
        'limits':'Offline command-channel start only; no native backend, physical START completion, TRE publication or functional DMA.'}
    manifest['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)} for p in OUT.iterdir() if p.is_file() and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'PASS: {groups} command-start groups, 9 exact full Linux start writes, {len(mutations)} rejected mutations, AArch64 no_std; {OUT}')
if __name__=='__main__':main()
