#!/usr/bin/env python3
"""Bind the retained second transaction and compile its closed native whitelist."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,re,subprocess
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-transfer'
def compact(s):return re.sub(r'\s+','',s)
def check(entry,parent,core,pool,wrapper):
    for token in ['uc::Io::guard(&mut retained)', 'pool: Some(pool)', 'for w in ep::PROFILE',
        'self.memory.report().phase != dma::Phase::Complete','self.memory.report().error.is_some()',
        '!self.memory.report().ack_done','self.memory.report().containment_sent','!pool.memory_setup_complete()',
        'memory: state._transfer.as_ref()?', 'state._tables = Some(TableOnce::new(hz).unwrap())',
        'once.begin(setup, endpoints, command, memory, pool, io)',
        'o.notified(p, i, now)','o.timeout(p, i, now)','o.external_fault(p, i)','o.late_fault(p, i)',
        'if badge != 1 << pixel_ipa_gsi::CHANNEL','let badge = receive();','if badge != 0',
        'sel4::Cap::<sel4::cap_type::Endpoint>::from_bits(1)',
        'sel4::Cap::<sel4::cap_type::Reply>::from_bits(4)','.nb_recv(',
        'cntpct_el0','cntfrq_el0','1_000_000..=1_000_000_000',
        'if !allowed(r.register(), r.value())','dma::Register::LocalContext => false',
        'let base = self.registers.gsi;', 'offset >= buffers::ZERO_BYTES','offset % 4 != 0',
        'pixel_ipa_command::quarantine(state)','Channel::new(pixel_ipa_gsi::CHANNEL)',
        'base.cast_mut()','.write_volatile(r.value().to_le())','dsb sy']:
        if token not in entry:raise ValueError('native table binding changed: '+token)
    if entry.count('.write_volatile(')!=1 or 'memory_region_symbol!' in entry:raise ValueError('new native table store/alias path')
    if entry.index('state._tables = Some(')>entry.index('once.begin(setup, endpoints, command, memory, pool, io)'):raise ValueError('unretained table transaction')
    for token in ['mod pixel_ipa_tables;', 'pixel_ipa_tables::start(&d, &mut state)',
        'pixel_ipa_tables::late_notification(self)',
        '_tables: Option<personal_os_rust_server::sunfish_ipa_linux_table_transfer::TableOnce>']:
        if token not in parent:raise ValueError('parent table binding changed: '+token)
    if parent.index('pixel_ipa_transfer::start(&d, &mut state)')>parent.index('pixel_ipa_tables::start(&d, &mut state)'):raise ValueError('table startup precedes memory')
    for token in ['#![forbid(unsafe_code)]','pub(crate) fn new_tables', 'pool.claim_table_setup()',
        '!owner.memory_completed()', '!pool.memory_setup_complete()',
        'self.table_plan.is_some() != memory.is_some()', 'self.report.table_source_words += 1',
        'plan.table_bytes().chunks_exact(4)', 'pool.table_setup_write32(offset + index * 4, v)',
        'p.completion(pointer, bytes)', 'pool.finish_table_setup()',
        'if self.table_plan.is_none()', 'self.targets(pool, io, false)?']:
        if compact(token) not in compact(core):raise ValueError('table coordinator changed: '+token)
    writer=pool.split('pub(crate) fn table_setup_write32(',1)[1].split('pub(crate) fn finish_table_setup(',1)[0]
    for token in ['!self.memory_setup_complete()', '!self.table_setup_claimed','self.table_setup_complete',
        'payload.offset + 4 * COMMAND_ELEMENT_BYTES','payload.offset + 10 * COMMAND_ELEMENT_BYTES',
        'tre.offset + 4 * RING_ELEMENT_BYTES','tre.offset + 10 * RING_ELEMENT_BYTES', '|| !permitted']:
        if compact(token) not in compact(writer):raise ValueError('table pool boundary changed: '+token)
    for token in ['#![forbid(unsafe_code)]','pub struct TableOnce','inner: TransferOnce',
        'TransferOnce::new_tables(timeout_ticks)', 'memory: &TransferOnce', 'self.inner.begin_tables(']:
        if compact(token) not in compact(wrapper):raise ValueError('table owner boundary changed: '+token)
    if 'pub inner:' in wrapper:raise ValueError('table engine exposed')
def audit():
    paths=['rust/src/pixel_ipa_tables.rs','rust/src/sunfish_native_startup_preflight.rs',
        'rust/src/sunfish_ipa_linux_transfer.rs','rust/src/sunfish_ipa_linux_buffers.rs','rust/src/sunfish_ipa_linux_table_transfer.rs']
    texts=[(ROOT/p).read_text()for p in paths];check(*texts)
    mutations=[(0,'if !allowed(r.register(), r.value())','if false'),
        (0,'uc::Io::guard(&mut retained)','Ok::<(), uc::Error>(())'),
        (0,'state._tables = Some(','let unretained = Some('),
        (0,'if badge != 1 << pixel_ipa_gsi::CHANNEL','if false'),
        (0,'|| !pool.memory_setup_complete()','|| false'),
        (2,'pool.claim_table_setup()','Ok::<(), buffers::MemoryError>(())'),
        (2,'self.report.table_source_words += 1','self.report.table_source_words += 0'),
        (3,'payload.offset + 10 * COMMAND_ELEMENT_BYTES','payload.offset + 11 * COMMAND_ELEMENT_BYTES'),
        (4,'memory: &TransferOnce','memory: &Report')]
    for i,old,new in mutations:
        assert texts[i].count(old)==1,(i,old)
        v=texts.copy();v[i]=v[i].replace(old,new)
        try:check(*v)
        except ValueError:continue
        raise ValueError('native table mutation accepted: '+old)
    pure='fn allowed('+texts[0].split('fn allowed(',1)[1].split('fn counter()',1)[0]
    names=['sunfish_ipa_linux_buffers','sunfish_ipa_linux_config','sunfish_ipa_linux_memory',
        'sunfish_ipa_linux_memsetup','sunfish_ipa_linux_gsi','sunfish_ipa_linux_endpoint',
        'sunfish_ipa_linux_command','sunfish_ipa_linux_tables','sunfish_ipa_linux_transfer','sunfish_ipa_linux_table_transfer']
    harness='\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};'for n in names)
    harness+='\nuse sunfish_ipa_linux_transfer as dma;\n'+pure+r'''
#[test]
fn actual_native_table_whitelist_matches_compiled_c_and_rejects_other_words(){
 let regs=[dma::Register::ChannelDoorbell,dma::Register::EventDoorbell,dma::Register::IeobClear,
  dma::Register::IeobMask,dma::Register::TypeMask,dma::Register::LocalContext];
 for line in std::fs::read_to_string(std::env::var("TABLE_C").unwrap()).unwrap().lines(){
  let p:Vec<_>=line.split_whitespace().collect();let o=usize::from_str_radix(p[0],16).unwrap();let v=u32::from_str_radix(p[1],16).unwrap();
  let r=*regs.iter().find(|r|r.offset()==o).unwrap();assert!(allowed(r,v));
 }
 for r in regs {for v in (0..=65535).chain([0x80004040,0x800040a0,0x80003000,0x80003010,0x80003020,0x80004090,u32::MAX]) {
  let expected=match r {dma::Register::ChannelDoorbell=>v==0x800040a0,dma::Register::EventDoorbell=>v==0x80003010,
   dma::Register::IeobClear=>v==1,dma::Register::IeobMask=>v<=1,dma::Register::TypeMask=>[0,0x44,0x4c].contains(&v),dma::Register::LocalContext=>false};
  assert_eq!(allowed(r,v),expected);
 }}
}
'''
    p=OUT/'native-table-whitelist.rs';p.write_text(harness);binary=OUT/'native-table-whitelist'
    env=dict(os.environ,TABLE_C=str(OUT/'linux-table-transfer-writes.txt'))
    for label,args in [('native-table-whitelist-build',['rustc','--edition=2024','-D','warnings','--test',str(p),'-o',str(binary)]),('native-table-whitelist',[str(binary)])]:
        r=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True);(OUT/(label+'.log')).write_text(r.stdout+r.stderr)
        if r.returncode:raise ValueError(label+' failed; see private log')
    spec=importlib.util.spec_from_file_location('tables_transfer_audit',HERE/'audit-native-ipa-transfer-source.py')
    base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base);result=base.audit()
    result['native_tables']={'source_sha256':dict(zip(paths,[hashlib.sha256(t.encode()).hexdigest()for t in texts])),
        'mutations_rejected':len(mutations),'compiled_native_whitelist':True,'table_commands':6,'channel':1,'event':0,
        'fresh_tre_slots':[4,5,6,7,8,9],'event_slot':1,'timeout_seconds':1,'counter':'cntpct_el0',
        'startup_irq':464,'microkit_channel':3,'new_frames_or_irqs':False,
        'scope':'Second fixed table transaction after actual memory DMA; no MPSS/WLAN or flash',
        'functional_table_dma_proven':False,'production_ready':False}
    return result
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.write_text(json.dumps(audit(),indent=2)+'\n');print('Native table owners, closed writes, IRQ and counter bindings pass; 9 new mutations rejected.')
