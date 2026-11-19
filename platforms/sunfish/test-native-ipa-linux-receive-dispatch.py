#!/usr/bin/env python3
"""Shared IPA/GSI retained dispatch and original Linux UC/suspend source order."""
from pathlib import Path
import hashlib,importlib.util,json,os,re,shutil,subprocess,tarfile
ROOT=Path(__file__).resolve().parents[2];HERE=Path(__file__).resolve().parent
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-receive-dispatch'
spec=importlib.util.spec_from_file_location('receive_runtime_proof',HERE/'test-native-ipa-linux-receive-runtime.py')
rx=importlib.util.module_from_spec(spec);spec.loader.exec_module(rx);rx.OUT=OUT
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,label,env=None,success=True):return rx.run(args,label,env,success)
def oracle():
    archive=ROOT/'.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive)!=rx.ARCHIVE_HASH:raise ValueError('Wrong pinned source archive')
    with tarfile.open(archive)as t:
        source=t.extractfile('linux-'+rx.PIN+'/drivers/net/ipa/ipa_interrupt.c').read().decode()
    (OUT/'original-ipa-interrupt.c').write_text(source)
    template=rx.verified_fixture('ipa-linux-receive','replenish-oracle.c').read_text()
    c=template.split('#define NET_SKB_PAD',1)[0]+r'''
#ifndef DIV_ROUND_UP
#define DIV_ROUND_UP(n,d) (((n)+(d)-1)/(d))
#endif
#ifndef fallthrough
#define fallthrough __attribute__((fallthrough))
#endif
#ifndef dev_dbg
#define dev_dbg(...) ((void)0)
#endif
typedef int irqreturn_t;
#define IRQ_HANDLED 1
struct device { int unused; };
struct ipa { enum ipa_version version; void *reg_virt; struct device *dev; u32 endpoint_count; };
struct ipa_interrupt { struct ipa *ipa; u32 enabled; };
static u8 mmio[0x20000];static u32 registers[0x20000/4],pm_get,pm_put;
const struct reg *ipa_reg(struct ipa *i,enum ipa_reg_id id){(void)i;return reg(&ipa_regs_v4_2,id);}
static int pm_runtime_get_sync(struct device *d){(void)d;pm_get++;return 0;}
static int pm_runtime_put_autosuspend(struct device *d){(void)d;pm_put++;return 0;}
static u32 ioread32(void *p){size_t o=(u8*)p-mmio;u32 v=registers[o/4];if(o==0x3030)printf("info %08x\n",v);return v;}
static void iowrite32(u32 v,void *p){size_t o=(u8*)p-mmio;printf("write %05lx %08x\n",(unsigned long)o,v);
 if(o==0x3010){registers[0x3008/4]&=~v;if(v&0x4000)registers[0x3030/4]=0;}
 else if(o==0x3038)registers[0x3030/4]&=~v;else abort();}
static void ipa_uc_interrupt_handler(struct ipa *i,u32 id){(void)i;if(registers[0x3008/4]&BIT(id))abort();
 if(id==IPA_IRQ_UC_0)puts("byte 20 2");else if(id==IPA_IRQ_UC_1)puts("byte 12 1");else abort();}
'''
    for name in ['ipa_interrupt_suspend_clear_all','ipa_interrupt_process','ipa_isr_thread']:
        m=re.search(r'^static (?:void|irqreturn_t) '+name+r'\([^;]*?\)\n\{',source,re.M)
        if not m:raise ValueError('Missing original function: '+name)
        end=m.end();depth=1
        while depth:depth+=(source[end]=='{')-(source[end]=='}');end+=1
        c+=source[m.start():end]+'\n'
    c+=r'''
int main(void){struct device dev={0};struct ipa i={.version=IPA_VERSION_4_2,.reg_virt=mmio,.dev=&dev,.endpoint_count=17};
 struct ipa_interrupt intr={.ipa=&i,.enabled=0x400c};registers[0x3008/4]=0x1408c;registers[0x3030/4]=0x10100;
 if(reg_offset(ipa_reg(&i,IPA_IRQ_CLR))!=0x3010||reg_n_offset(ipa_reg(&i,IRQ_SUSPEND_INFO),0)!=0x3030||reg_n_offset(ipa_reg(&i,IRQ_SUSPEND_CLR),0)!=0x3038)abort();
 if(ipa_isr_thread(343,&intr)!=IRQ_HANDLED||registers[0x3008/4]||registers[0x3030/4]||pm_get!=1||pm_put!=1)abort();
 /* Microkit ACK is a separate platform boundary after handler success. */
 puts("ack");return 0;}
'''
    path=OUT/'linux-shared-ipa.c';path.write_text(c)
    run([shutil.which('clang')or'cc','-std=gnu11','-Wall','-Wextra','-Werror','-Wno-unused-function','-Wno-unused-const-variable','-Wno-unused-parameter',str(path),'-o',str(OUT/'linux-shared-ipa')],'c-build')
    text=run([str(OUT/'linux-shared-ipa')],'c-oracle').stdout
    golden=OUT/'linux-shared-ipa.txt';golden.write_text(text);return golden
def main():
    OUT.mkdir(parents=True,exist_ok=True,mode=0o700);golden=oracle()
    env=dict(os.environ,SUNFISH_RECEIVE_DISPATCH_C=str(golden),SUNFISH_RECEIVE_RUNTIME_C=str(rx.verified_fixture('ipa-linux-receive-runtime','linux-receive-runtime.txt')))
    fixtures={'SUNFISH_RECEIVE_OWNERS_FIXTURE':('ipa-linux-receive-start','retained-fixture.rs'),
        'SUNFISH_RECEIVE_STARTUP_C':('ipa-linux-receive-start','linux-receive-startup-writes.txt'),
        'SUNFISH_TRANSFER_START_FIXTURE':('ipa-linux-transfer','start-fixture.rs'),
        'SUNFISH_COMMAND_ENDPOINT_FIXTURE':('ipa-linux-command','endpoint-fixture.rs'),
        'SUNFISH_ENDPOINT_GSI_FIXTURE':('ipa-linux-endpoint','gsi-fixture.rs'),
        'SUNFISH_TRANSFER_C_REFERENCE':('ipa-linux-transfer','linux-transfer-writes.txt'),
        'SUNFISH_TABLE_TRANSFER_C_REFERENCE':('ipa-linux-transfer','linux-table-transfer-writes.txt'),
        'SUNFISH_COMMAND_C_REFERENCE':('ipa-linux-command','linux-command-writes.txt')}
    for k,(folder,name)in fixtures.items():env[k]=str(rx.verified_fixture(folder,name))
    rustc=shutil.which('rustc');test=ROOT/'tests/sunfish_ipa_linux_receive_dispatch_test.rs';binary=OUT/'shared-irq-test'
    run([rustc,'--edition=2024','-D','warnings','-C','opt-level=2','--test',str(test),'-o',str(binary)],'rust-build',env)
    tested=run([str(binary),'sharedirq_'],'rust-tests',env)
    groups=int(re.search(r'test result: ok\. (\d+) passed;',tested.stdout)[1])
    run([str(binary),'rxruntime_'],'receive-runtime-regression',env)
    run([str(binary),'retained_setup_requires_a_successful_live_guard'], 'retained-setup', env)
    permit_binary=OUT/'shared-irq-permit-test'
    run([rustc,'--edition=2024','-D','warnings','-A','dead_code','-C','opt-level=2',
         '--cfg','feature="sunfish-linux-ipa-qmi"','--test',str(test),'-o',str(permit_binary)],
        'permit-build',env)
    run([str(permit_binary),'linux_ap_permit_requires_live_guard_and_binds_receipt_generation'],
        'permit-tests',env)
    uc_test=OUT/'uc-regression-test'
    run([rustc,'--edition=2024','-D','warnings','--test',str(ROOT/'tests/sunfish_ipa_linux_uc_test.rs'),'-o',str(uc_test)],'uc-regression-build')
    run([str(uc_test)],'uc-regression')
    names=rx.NAMES+['irq','uc','receive_dispatch']
    wrapper=OUT/'core.rs';wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT}/rust/src/sunfish_ipa_linux_{n}.rs"] pub mod sunfish_ipa_linux_{n};'for n in names))
    run([rustc,'--edition=2024','-D','warnings','--crate-type=lib','--target=aarch64-unknown-none',str(wrapper),'-o',str(OUT/'shared-irq-aarch64.rlib')],'aarch64-core')
    original=(ROOT/'rust/src/sunfish_ipa_linux_receive_dispatch.rs').read_text()
    mutants={'replay-ipa-mask':('self.receive.ipa_dispatch_failed(io);','let _ = self.receive.external_fault(io);'),
        'ignore-foreign-fault':('if notice == Notice::Fault {','if false && notice == Notice::Fault {'),
        'skip-gsi-fault-priority':('self.receive.check_interrupt_sources(io)','Ok::<(), rx::Error>(())')}
    mutated_sources=[]
    for label,(before,after)in mutants.items():
        if original.count(before)!=1:raise ValueError('Mutation not unique: '+before)
        mutated_sources.append((label,'receive_dispatch',original.replace(before,after)))
    uc=(ROOT/'rust/src/sunfish_ipa_linux_uc.rs').read_text()
    before='let info = io.suspend_info()?;'
    if uc.count(before)!=1:raise ValueError('Suspend mutation not unique')
    mutated_sources.append(('clear-suspend-source-first','uc',uc.replace(before,'self.clear(io, 1 << 14)?;\n                '+before)))
    prefix,tail=uc.split('pub(crate) struct ReceiveRuntime',1)
    if tail.count('self.clear(io, mask)?;')!=1:raise ValueError('UC receive mutation not unique')
    tail=tail.replace('self.clear(io, mask)?;','/* clear deliberately deferred */').replace('let value = io.shared(field)?;','let value = io.shared(field)?;\n                    self.clear(io, mask)?;')
    mutated_sources.append(('read-uc-before-clear','uc',prefix+'pub(crate) struct ReceiveRuntime'+tail))
    for label,module,changed in mutated_sources:
        p=OUT/(label+'.rs');p.write_text(changed)
        body=test.read_text().replace('include!("sunfish_ipa_linux_receive_runtime_test.rs");',f'include!("{ROOT}/tests/sunfish_ipa_linux_receive_runtime_test.rs");')
        for name in ['irq','uc','receive_dispatch']:
            target=p if name==module else ROOT/'rust/src'/('sunfish_ipa_linux_'+name+'.rs')
            body=body.replace('../rust/src/sunfish_ipa_linux_'+name+'.rs',str(target))
        path=OUT/(label+'-test.rs');path.write_text(body);exe=OUT/(label+'-test')
        run([rustc,'--edition=2024','-D','warnings','-C','opt-level=2','--test',str(path),'-o',str(exe)],label+'-build',env)
        if run([str(exe),'sharedirq_'],label,env,False).returncode==0:raise ValueError('Mutation accepted: '+label)
    selected=['rust/src/sunfish_ipa_linux_'+n+'.rs'for n in names]+['rust/src/lib.rs','rust/Cargo.toml','rust/Cargo.lock',
        'tests/sunfish_ipa_linux_receive_dispatch_test.rs','tests/sunfish_ipa_linux_receive_runtime_test.rs','tests/sunfish_ipa_linux_receive_start_test.rs',
        'tests/sunfish_ipa_linux_uc_test.rs','platforms/sunfish/test-native-ipa-linux-receive-dispatch.py']
    for n in selected:
        path=OUT/'source'/n;path.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(ROOT/n,path)
    j={'source_sha256':{n:sha(ROOT/n)for n in selected},'kernel_commit':rx.PIN,'archive_sha256':rx.ARCHIVE_HASH,
       'behavioral_groups':groups,'receive_runtime_regression_groups':10,'legacy_uc_regression':True,
       'mutations_rejected':[label for label,_,_ in mutated_sources],'aarch64_no_std':True,'hardware_commands':[],
       'scope':'Actual retained receive/UC owner composition with original Linux IPA ISR and host MMIO, byte callbacks and ACK stand-ins. Concrete native entry/IRQ binding and physical RX remain unproven.'}
    j['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)}for p in OUT.iterdir()if p.is_file()and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(j,indent=2)+'\n')
    print(f'PASS: {groups} shared IPA/GSI groups, original Linux suspend/UC order, regressions, five rejected mutations and AArch64 no_std; no device action.')
if __name__=='__main__':main()
