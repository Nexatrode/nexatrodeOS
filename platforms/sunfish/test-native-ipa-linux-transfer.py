#!/usr/bin/env python3
"""Retained memory/table transactions; host simulation and pinned C IRQ oracle.

No device operations. The complete expected write list combines the three
compiled memsetup/ring writes with the original IEOB ISR and re-enable helper;
the chosen native poll-before-ACK policy is explicitly host-side glue.
"""
from pathlib import Path
import hashlib,importlib.util,json,os,re,shutil,subprocess

ROOT=Path(__file__).resolve().parents[2]
HERE=Path(__file__).resolve().parent
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-transfer'

def module(name,file):
    spec=importlib.util.spec_from_file_location(name,HERE/file);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def run(args,name,env=None,expected=0):
    p=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(name+'.log')).write_text(p.stdout+p.stderr)
    if p.returncode!=expected:raise SystemExit(f'{name}: exit {p.returncode}; see {OUT/(name+".log")}')
    return p
def fresh(m):
    p=m.OUT/'manifest.json'
    if not p.exists() or any(sha(ROOT/n)!=h for n,h in json.loads(p.read_text())['source_sha256'].items()):m.main()

def main():
    OUT.mkdir(mode=0o700,parents=True,exist_ok=True)
    cmd=module('transfer_command','test-native-ipa-linux-command.py');fresh(cmd)
    mem=module('transfer_memsetup','test-native-ipa-linux-memsetup.py');fresh(mem)
    table=module('transfer_tables','test-native-ipa-linux-tables.py');fresh(table)
    g=module('transfer_gsi','test-native-ipa-linux-gsi.py')
    c=(cmd.OUT/'linux-command-oracle.c').read_text()
    replacements={
        'struct gsi_evt_ring { struct gsi_ring ring; };':'struct gsi_evt_ring { struct gsi_ring ring; struct gsi_channel *channel; };',
        'else if (offset == 0x1f0a4)':'else if (offset == 0x1f0c0) registers[0x1f0b0 / 4] &= ~value;\n    else if (offset == 0x1f0a4)',
    }
    for before,after in replacements.items():assert c.count(before)==1,before;c=c.replace(before,after)
    s=(cmd.OUT/'linux-source'/g.IPA/'gsi.c').read_text()
    extra='static u32 scheduled;\nstatic void napi_schedule(struct napi_struct *n) { (void)n; scheduled++; }\n'+g.function(s,'gsi_isr_ieob')
    c=c.replace('int main(void) {',extra+'\nint main(void) {',1)
    before,tail=c.rsplit('}',1)
    c=before+'''    setup_result = 4;
    g.evt_ring[0].channel = &g.channel[1];
    registers[0x1f0b0 / 4] = 1;
    gsi_isr_ieob(&g);
    if (scheduled != 1 || registers[0x1f0b0 / 4] || registers[0x1f088 / 4] != 0x44 || registers[0x1f0b8 / 4]) abort();
    gsi_irq_ieob_enable_one(&g, 0);
    if (registers[0x1f088 / 4] != 0x4c || registers[0x1f0b8 / 4] != 1) abort();
}'''+tail
    cp=OUT/'linux-transfer-irq-oracle.c';cp.write_text(c)
    cc=shutil.which('clang') or shutil.which('cc');assert cc
    run([cc,'-std=gnu11','-Wall','-Wextra','-Werror',str(cp),'-o',str(OUT/'linux-transfer-irq-oracle')],'c-build')
    rows=run([str(OUT/'linux-transfer-irq-oracle')],'c-oracle').stdout.splitlines()
    irq=[' '.join(r.split()[2:]) for r in rows if r.startswith('setup 4 ')]
    memory=[r.removeprefix('write ') for r in (mem.OUT/'linux-memsetup.txt').read_text().splitlines() if r.startswith('write ')]
    assert len(irq)==5 and len(memory)==3
    golden=OUT/'linux-transfer-writes.txt';golden.write_text('\n'.join([memory[0],*irq[:3],memory[1],*irq[3:],memory[2]])+'\n')
    table_writes=[r.removeprefix('write ') for r in (table.OUT/'linux-tables.txt').read_text().splitlines() if r.startswith('write ')]
    assert len(table_writes)==2
    table_golden=OUT/'linux-table-transfer-writes.txt'
    table_golden.write_text('\n'.join([table_writes[0],*irq[:3],table_writes[1],*irq[3:]])+'\n')
    body=(ROOT/'tests/sunfish_ipa_linux_command_test.rs').read_text()
    body='use crate::{sunfish_ipa_linux_command,sunfish_ipa_linux_endpoint,sunfish_ipa_linux_gsi};\n'+body[body.index('mod fixture'):]
    body+='\npub fn started()->(gsi::SetupOnce,ep::ConfigureOnce,StartOnce) {let(g,e)=fixture::owned_startup();let mut io=Fake::new();let mut c=StartOnce::new(50).unwrap();run(&mut c,&g,&e,&mut io).unwrap();(g,e,c)}\n'
    fixture=OUT/'start-fixture.rs';fixture.write_text(body)
    ep=module('transfer_endpoint','test-native-ipa-linux-endpoint.py')
    env=dict(os.environ,SUNFISH_TRANSFER_START_FIXTURE=str(fixture),SUNFISH_COMMAND_ENDPOINT_FIXTURE=str(cmd.OUT/'endpoint-fixture.rs'),
        SUNFISH_ENDPOINT_GSI_FIXTURE=str(ep.OUT/'gsi-fixture.rs'),SUNFISH_TRANSFER_C_REFERENCE=str(golden),
        SUNFISH_TABLE_TRANSFER_C_REFERENCE=str(table_golden),
        SUNFISH_COMMAND_C_REFERENCE=str(cmd.OUT/'linux-command-writes.txt'))
    rustc=shutil.which('rustc');assert rustc
    binary=OUT/'transfer-test'
    run([rustc,'--edition=2024','-D','warnings','-C','opt-level=2','--test',str(ROOT/'tests/sunfish_ipa_linux_transfer_test.rs'),'-o',str(binary)],'rust-build',env)
    tested=run([str(binary),'dma_'],'rust-tests',env)
    groups=int(re.search(r'test result: ok\. (\d+) passed;',tested.stdout)[1])
    table_groups=len(re.findall(r'^test dma_tables_.* \.\.\. ok$',tested.stdout,re.M))
    names=['sunfish_ipa_linux_config','sunfish_ipa_linux_memory','sunfish_ipa_linux_buffers','sunfish_ipa_linux_memsetup',
        'sunfish_ipa_linux_gsi','sunfish_ipa_linux_endpoint','sunfish_ipa_linux_command','sunfish_ipa_linux_tables',
        'sunfish_ipa_linux_transfer','sunfish_ipa_linux_table_transfer']
    wrapper=OUT/'core.rs';wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names))
    run([rustc,'--edition=2024','-D','warnings','--crate-type=lib','--target=aarch64-unknown-none',str(wrapper),'-o',str(OUT/'transfer-aarch64.rlib')],'aarch64-core')
    selected=['rust/src/'+n+'.rs' for n in names]+['rust/src/lib.rs','platforms/sunfish/test-modem-qmi.py',
        'tests/sunfish_ipa_linux_transfer_test.rs','platforms/sunfish/test-native-ipa-linux-transfer.py']
    for n in selected:
        p=OUT/'source'/n;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/n,p)
    manifest={'kernel_commit':g.PIN,'source_sha256':{n:sha(ROOT/n) for n in selected},
        'command_prerequisite_manifest_sha256':sha(cmd.OUT/'manifest.json'),'memory_encoder_manifest_sha256':sha(mem.OUT/'manifest.json'),
        'table_encoder_manifest_sha256':sha(table.OUT/'manifest.json'),
        'compiled_c_comparison_writes':8,'compiled_c_table_comparison_writes':7,
        'behavioral_groups':groups,'memory_behavioral_groups':groups-table_groups,'table_behavioral_groups':table_groups,
        'aarch64_no_std':True,'hardware_commands':[],
        'limits':'Host simulation only. No native hardware backend, physical DMA/IRQ proof or production readiness.'}
    manifest['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)} for p in OUT.iterdir() if p.is_file() and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'PASS: {groups-table_groups} memory / {table_groups} table behavioral groups, 8 / 7 compiled-C-composed writes, AArch64 no_std; {OUT}')

if __name__=='__main__':main()
