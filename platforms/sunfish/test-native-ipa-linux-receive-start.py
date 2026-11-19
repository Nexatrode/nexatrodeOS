#!/usr/bin/env python3
"""Retained RX startup with actual prior core owners, host-only backends."""
from pathlib import Path
import hashlib,importlib.util,json,os,re,shutil,subprocess
ROOT=Path(__file__).resolve().parents[2]
HERE=Path(__file__).resolve().parent
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-receive-start'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,name,env=None):
    p=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(name+'.log')).write_text(p.stdout+p.stderr)
    if p.returncode:raise SystemExit(f'{name}: exit {p.returncode}; see {OUT/(name+".log")}')
    return p
def main():
    OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    dependency=HERE/'test-native-ipa-linux-transfer.py'
    dep_spec=importlib.util.spec_from_file_location('receive_transfer',dependency)
    transfer=importlib.util.module_from_spec(dep_spec);dep_spec.loader.exec_module(transfer)
    proof=transfer.OUT/'manifest.json'
    if not proof.exists()or any(sha(ROOT/n)!=h for n,h in json.loads(proof.read_text())['source_sha256'].items()):transfer.main()
    spec=importlib.util.spec_from_file_location('receive_planner',HERE/'test-native-ipa-linux-receive.py')
    rx=importlib.util.module_from_spec(spec);spec.loader.exec_module(rx)
    p=rx.OUT/'manifest.json'
    if not p.exists()or any(sha(ROOT/n)!=h for n,h in json.loads(p.read_text())['source_sha256'].items()):rx.main()
    rows=['gsi '+r for r in (rx.OUT/'linux-receive-start.txt').read_text().splitlines()]
    for row in (rx.OUT/'linux-receive-replenish.txt').read_text().splitlines():
        if row.startswith('write '):
            _,offset,value=row.split();rows.append(('ipa 'if int(offset,16)<0x10000 else 'gsi ')+offset+' '+value)
    assert len(rows)==27
    golden=OUT/'linux-receive-startup-writes.txt';golden.write_text('\n'.join(rows)+'\n')
    old=(ROOT/'tests/sunfish_ipa_linux_transfer_test.rs').read_text()
    aliases=['sunfish_ipa_linux_buffers','sunfish_ipa_linux_command','sunfish_ipa_linux_endpoint','sunfish_ipa_linux_gsi',
        'sunfish_ipa_linux_memory','sunfish_ipa_linux_memsetup','sunfish_ipa_linux_transfer','sunfish_ipa_linux_tables','sunfish_ipa_linux_table_transfer']
    body='use crate::{'+','.join(aliases)+'};\n'+old[old.index('mod owners {'):]
    body=body.replace('struct Ram(Rc<RefCell<RamState>>);','pub struct Ram(Rc<RefCell<RamState>>);')
    body=body.replace('type Owners =','pub type Owners =')
    body+='''
pub struct Retained {pub owners:Owners,pub memory:TransferOnce,pub tables:TableOnce,pub pool:Option<Pool<Ram>>,pub ram:Ram}
pub fn completed()->Retained {
 let own=owners::started();let(memory,mut pool,mut io)=table_fixture(&own);
 let mut tables=TableOnce::new(1000).unwrap();run_table(&mut tables,&own,&memory,&mut pool,&mut io).unwrap();
 Retained{owners:own,memory,tables,pool:Some(pool),ram:io.ram.clone()}
}
pub fn write_word(r:&Ram,o:usize,v:u32){r.0.borrow_mut().words[o/4]=v;}
pub fn snapshot(r:&Ram)->Vec<u32>{r.0.borrow().words.clone()}
pub fn fail_read_after(r:&Ram,n:usize){let mut s=r.0.borrow_mut();s.fail_read=Some(s.reads+n);}
pub fn write_count(r:&Ram)->usize{r.0.borrow().writes}
pub fn fail_write_after(r:&Ram,n:usize){let mut s=r.0.borrow_mut();s.fail_write=Some(s.writes+n);}
pub fn suppress_write_after(r:&Ram,n:usize){let mut s=r.0.borrow_mut();s.suppress=Some(s.writes+n);}
pub fn memory_only_pool()->Pool<Ram>{let own=owners::started();let(_,pool,_)=table_fixture(&own);pool}

'''
    fixture=OUT/'retained-fixture.rs';fixture.write_text(body)
    base=ROOT/'.cache/sunfish-native-wifi/private'
    env=dict(os.environ,SUNFISH_RECEIVE_OWNERS_FIXTURE=str(fixture),SUNFISH_RECEIVE_STARTUP_C=str(golden),
        SUNFISH_TRANSFER_START_FIXTURE=str(base/'ipa-linux-transfer/start-fixture.rs'),
        SUNFISH_COMMAND_ENDPOINT_FIXTURE=str(base/'ipa-linux-command/endpoint-fixture.rs'),
        SUNFISH_ENDPOINT_GSI_FIXTURE=str(base/'ipa-linux-endpoint/gsi-fixture.rs'),
        SUNFISH_TRANSFER_C_REFERENCE=str(base/'ipa-linux-transfer/linux-transfer-writes.txt'),
        SUNFISH_TABLE_TRANSFER_C_REFERENCE=str(base/'ipa-linux-transfer/linux-table-transfer-writes.txt'),
        SUNFISH_COMMAND_C_REFERENCE=str(base/'ipa-linux-command/linux-command-writes.txt'))
    for key in ['SUNFISH_TRANSFER_START_FIXTURE','SUNFISH_COMMAND_ENDPOINT_FIXTURE','SUNFISH_ENDPOINT_GSI_FIXTURE',
        'SUNFISH_TRANSFER_C_REFERENCE','SUNFISH_TABLE_TRANSFER_C_REFERENCE','SUNFISH_COMMAND_C_REFERENCE']:
        dependency=Path(env[key]);proof=json.loads((dependency.parent/'manifest.json').read_text())
        assert sha(dependency)==proof['artifacts'][dependency.name]['sha256'],key
    rustc=shutil.which('rustc');assert rustc
    binary=OUT/'receive-start-test'
    run([rustc,'--edition=2024','-D','warnings','-C','opt-level=2','--test',str(ROOT/'tests/sunfish_ipa_linux_receive_start_test.rs'),'-o',str(binary)],'rust-build',env)
    tested=run([str(binary),'rxstart_'],'rust-tests',env)
    groups=int(re.search(r'test result: ok\. (\d+) passed;',tested.stdout)[1])
    names=['sunfish_ipa_linux_buffers','sunfish_ipa_linux_config','sunfish_ipa_linux_gsi','sunfish_ipa_linux_endpoint',
        'sunfish_ipa_linux_command','sunfish_ipa_linux_memory','sunfish_ipa_linux_memsetup','sunfish_ipa_linux_tables',
        'sunfish_ipa_linux_transfer','sunfish_ipa_linux_table_transfer','sunfish_ipa_linux_receive','sunfish_ipa_linux_receive_pool','sunfish_ipa_linux_receive_start']
    wrapper=OUT/'core.rs';wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};'for n in names))
    run([rustc,'--edition=2024','-D','warnings','--crate-type=lib','--target=aarch64-unknown-none',str(wrapper),'-o',str(OUT/'receive-start-aarch64.rlib')],'aarch64-core')
    selected=['rust/src/'+n+'.rs'for n in names]+['tests/sunfish_ipa_linux_receive_start_test.rs',
        'tests/sunfish_ipa_linux_transfer_test.rs','platforms/sunfish/test-native-ipa-linux-receive-start.py','docs/sunfish-ipa-receive-plan.md']
    for rel in selected:
        p=OUT/'source'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/rel,p)
    manifest={'source_sha256':{rel:sha(ROOT/rel)for rel in selected},'receive_comparison_manifest_sha256':sha(rx.OUT/'manifest.json'),
        'behavioral_groups':groups,'compiled_c_composed_writes':27,'actual_prior_core_owners':True,
        'aarch64_no_std':True,'hardware_commands':[],'native_backend':False,'library_exported':False,
        'scope':'Retained initial RX startup through the original bounded pool backend with host MMIO; no actual RX allocation/mapping, physical START or packet/completion proof.'}
    manifest['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)}for p in OUT.iterdir()if p.is_file()and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'PASS: {groups} retained RX startup groups, 27 C-composed writes, actual prior owners and AArch64 no_std; {OUT}')
if __name__=='__main__':main()
