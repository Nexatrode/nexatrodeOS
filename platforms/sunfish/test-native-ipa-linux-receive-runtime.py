#!/usr/bin/env python3
"""Actual retained RX owners, bounded packet reads and original Linux IEOB ISR."""
from pathlib import Path
import hashlib,json,os,re,shutil,subprocess,tarfile
ROOT=Path(__file__).resolve().parents[2]
PRIVATE=ROOT/'.cache/sunfish-native-wifi/private'
OUT=PRIVATE/'ipa-linux-receive-runtime'
PIN='2267e248204e7e051e0f7355298949a6440de579'
ARCHIVE_HASH='d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'
NAMES=['buffers','config','gsi','endpoint','command','memory','memsetup','tables','transfer','table_transfer',
       'receive','receive_pool','receive_start','receive_events','receive_runtime']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(argv,label,env=None,success=True):
    r=subprocess.run(argv,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(label+'.log')).write_text(r.stdout+r.stderr)
    if success and r.returncode:raise SystemExit(f'{label}: exit {r.returncode}; see {OUT/(label+".log")}')
    return r
def verified_fixture(folder,name):
    p=PRIVATE/folder/name;j=json.loads((p.parent/'manifest.json').read_text())
    if sha(p)!=j['artifacts'][name]['sha256']:raise ValueError('Changed fixture: '+str(p))
    dest=OUT/'fixtures'/folder/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
    return dest
def function(source,name):
    m=re.search(r'^static void '+name+r'\([^;]*?\)\n\{',source,re.M)
    if not m:raise ValueError('Missing original function: '+name)
    end=m.end();depth=1
    while depth:
        depth+=(source[end]=='{')-(source[end]=='}');end+=1
    return source[m.start():end]+'\n'
def oracle():
    archive=ROOT/'.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive)!=ARCHIVE_HASH:raise ValueError('Wrong pinned kernel archive')
    with tarfile.open(archive)as t:
        member=next(m for m in t.getmembers()if m.name.endswith('/drivers/net/ipa/gsi.c'))
        original=t.extractfile(member).read().decode()
    (OUT/'original-gsi.c').write_text(original)
    template=verified_fixture('ipa-linux-command','linux-command-oracle.c').read_text()
    c=template.split('struct napi_struct {',1)[0]
    c+=r'''
struct napi_struct { int unused; };
struct gsi_channel { struct napi_struct napi; };
struct gsi_evt_ring { struct gsi_channel *channel; };
struct gsi { void *virt; u32 type_enabled_bitmap; u32 ieob_enabled_bitmap;
    struct gsi_evt_ring evt_ring[31]; };
static u8 mmio[0x2c000];
static u32 registers[0x2c000/4], scheduled;
const struct reg *gsi_reg(struct gsi *gsi,enum gsi_reg_id id){(void)gsi;return reg(&gsi_regs_v4_0,id);}
static u32 ioread32(void *p){return registers[((u8*)p-mmio)/4];}
static void iowrite32(u32 v,void *p){size_t o=(u8*)p-mmio;printf("%05lx %08x\n",(unsigned long)o,v);
    if(o==0x1f0c0)registers[0x1f0b0/4]&=~v;else registers[o/4]=v;}
static void napi_schedule(struct napi_struct *n){if(!n)abort();scheduled++;}
'''
    for name in ['gsi_irq_type_update','gsi_irq_type_enable','gsi_irq_type_disable',
                 'gsi_irq_ieob_enable_one','gsi_irq_ieob_disable','gsi_isr_ieob']:
        c+=function(original,name)
    c+=r'''
int main(void){struct gsi_channel channel={0};struct gsi g={.virt=mmio,.type_enabled_bitmap=0x4c,.ieob_enabled_bitmap=3};
 g.evt_ring[1].channel=&channel;registers[0x1f088/4]=0x4c;registers[0x1f0b8/4]=3;registers[0x1f0b0/4]=2;
 gsi_isr_ieob(&g);
 if(g.type_enabled_bitmap!=0x4c||g.ieob_enabled_bitmap!=1||registers[0x1f0b0/4]||scheduled!=1)abort();
 gsi_irq_ieob_enable_one(&g,1);
 if(g.type_enabled_bitmap!=0x4c||g.ieob_enabled_bitmap!=3||registers[0x1f088/4]!=0x4c||registers[0x1f0b8/4]!=3)abort();
 return 0;}
'''
    p=OUT/'linux-receive-runtime.c';p.write_text(c)
    run([shutil.which('clang')or'cc','-std=gnu11','-Wall','-Wextra','-Werror','-Wno-unused-function','-Wno-unused-const-variable',str(p),'-o',str(OUT/'linux-receive-runtime')],'c-build')
    rows=run([str(OUT/'linux-receive-runtime')],'c-oracle').stdout
    if len(rows.splitlines())!=3:raise ValueError('Unexpected original ISR write count')
    golden=OUT/'linux-receive-runtime.txt';golden.write_text(rows);return golden
def main():
    OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    golden=oracle()
    fixtures={
        'SUNFISH_RECEIVE_OWNERS_FIXTURE':('ipa-linux-receive-start','retained-fixture.rs'),
        'SUNFISH_RECEIVE_STARTUP_C':('ipa-linux-receive-start','linux-receive-startup-writes.txt'),
        'SUNFISH_TRANSFER_START_FIXTURE':('ipa-linux-transfer','start-fixture.rs'),
        'SUNFISH_COMMAND_ENDPOINT_FIXTURE':('ipa-linux-command','endpoint-fixture.rs'),
        'SUNFISH_ENDPOINT_GSI_FIXTURE':('ipa-linux-endpoint','gsi-fixture.rs'),
        'SUNFISH_TRANSFER_C_REFERENCE':('ipa-linux-transfer','linux-transfer-writes.txt'),
        'SUNFISH_TABLE_TRANSFER_C_REFERENCE':('ipa-linux-transfer','linux-table-transfer-writes.txt'),
        'SUNFISH_COMMAND_C_REFERENCE':('ipa-linux-command','linux-command-writes.txt')}
    env=dict(os.environ,SUNFISH_RECEIVE_RUNTIME_C=str(golden))
    for key,(folder,name)in fixtures.items():env[key]=str(verified_fixture(folder,name))
    rustc=shutil.which('rustc');assert rustc
    binary=OUT/'receive-runtime-test';test=ROOT/'tests/sunfish_ipa_linux_receive_runtime_test.rs'
    run([rustc,'--edition=2024','-D','warnings','-C','opt-level=2','--test',str(test),'-o',str(binary)],'rust-build',env)
    tested=run([str(binary),'rxruntime_'],'rust-tests',env)
    groups=int(re.search(r'test result: ok\. (\d+) passed;',tested.stdout)[1])
    run([str(binary),'rxstart_'],'startup-regression',env)
    wrapper=OUT/'core.rs';wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT}/rust/src/sunfish_ipa_linux_{n}.rs"] pub mod sunfish_ipa_linux_{n};'for n in NAMES))
    run([rustc,'--edition=2024','-D','warnings','--crate-type=lib','--target=aarch64-unknown-none',str(wrapper),'-o',str(OUT/'receive-runtime-aarch64.rlib')],'aarch64-core')
    original=(ROOT/'rust/src/sunfish_ipa_linux_receive_runtime.rs').read_text()
    mutants={
        'ignore-packet-bound':('if end > self.completion.length() {','if end > rx::DMA_BYTES {'),
        'ignore-failed-read':('if let Some(e) = packet.error {','if let Some(e) = packet.error.filter(|_| false) {'),
        'clear-command-event':('access.write(Register::IeobClear, source)?;','access.write(Register::IeobClear, 1)?;'),
        'ack-before-work':('drained = match queue.drain(&mut access) {','access.io.acknowledge_receive()?;\n                drained = match queue.drain(&mut access) {')}
    for label,(before,after)in mutants.items():
        if original.count(before)!=1:raise ValueError('Mutation not unique: '+before)
        path=OUT/(label+'.rs');path.write_text(original.replace(before,after))
        harness=test.read_text().replace('include!("sunfish_ipa_linux_receive_start_test.rs");',f'include!("{ROOT}/tests/sunfish_ipa_linux_receive_start_test.rs");')
        harness=harness.replace('../rust/src/sunfish_ipa_linux_receive_events.rs',str(ROOT/'rust/src/sunfish_ipa_linux_receive_events.rs'))
        harness=harness.replace('../rust/src/sunfish_ipa_linux_receive_runtime.rs',str(path))
        mutated=OUT/(label+'-test.rs');mutated.write_text(harness);exe=OUT/(label+'-test')
        run([rustc,'--edition=2024','-D','warnings','-C','opt-level=2','--test',str(mutated),'-o',str(exe)],label+'-build',env)
        if run([str(exe),'rxruntime_'],label,env,False).returncode==0:raise ValueError('Mutation accepted: '+label)
    selected=['rust/src/sunfish_ipa_linux_'+n+'.rs'for n in NAMES]+[
        'rust/src/lib.rs','rust/Cargo.toml','rust/Cargo.lock',
        'tests/sunfish_ipa_linux_receive_runtime_test.rs','tests/sunfish_ipa_linux_receive_start_test.rs',
        'platforms/sunfish/test-native-ipa-linux-receive-runtime.py']
    for n in selected:
        p=OUT/'source'/n;p.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(ROOT/n,p)
    manifest={'kernel_commit':PIN,'archive_sha256':ARCHIVE_HASH,'source_sha256':{n:sha(ROOT/n)for n in selected},
        'behavioral_groups':groups,'startup_regression_groups':9,'original_linux_isr_writes':3,
        'mutations_rejected':list(mutants),'aarch64_no_std':True,'actual_prior_core_owners':True,
        'library_exported':all('pub mod sunfish_ipa_linux_'+n+';'in(ROOT/'rust/src/lib.rs').read_text()
            for n in ['receive_start','receive_events','receive_runtime']),
        'packet_reads_from_retained_completed_buffer_only':True,'hardware_commands':[],
        'scope':'Host RAM model of actual retained startup/event/buffer composition, CPU access and ordered GSI handling; physical native IRQ/RX integration and packet protocol parsing remain required. No device RX or Wi-Fi proof.'}
    manifest['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)}for p in OUT.iterdir()if p.is_file()and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'PASS: {groups} retained receive runtime groups, startup regression, original Linux ISR, four rejected mutations and AArch64 no_std; no device action.')
if __name__=='__main__':main()
