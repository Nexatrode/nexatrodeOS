#!/usr/bin/env python3
"""Bind one retained native memory transaction and compile its closed whitelist."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,re,subprocess
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-transfer'

def check(entry,parent,core,pool):
    for token in ['uc::Io::guard(&mut retained)', 'pool: Some(pool)', 'for w in ep::PROFILE',
        'self.command.report().phase != cmd::Phase::Complete','!self.command.report().ack_done',
        'self.setup.report().phase != gsi::SetupPhase::Complete',
        'state._transfer = Some(', 'once.begin(setup, endpoints, command, pool, io)',
        'o.notified(p, i, now)','o.timeout(p, i, now)','o.external_fault(p, i)','o.late_fault(p, i)',
        'if badge != 1 << pixel_ipa_gsi::CHANNEL','let badge = receive();','if badge != 0',
        'sel4::Cap::<sel4::cap_type::Endpoint>::from_bits(1)',
        'sel4::Cap::<sel4::cap_type::Reply>::from_bits(4)','.nb_recv(',
        'cntpct_el0','cntfrq_el0','1_000_000..=1_000_000_000',
        'dma::TransferOnce::new(hz)','if !allowed(r.register(), r.value())',
        'offset >= buffers::ZERO_BYTES','offset % 4 != 0',
        'pixel_ipa_command::quarantine(state)','Channel::new(pixel_ipa_gsi::CHANNEL)',
        'base.cast_mut()','.write_volatile(r.value().to_le())','dsb sy']:
        if token not in entry:raise ValueError('native transfer binding changed: '+token)
    if entry.count('.write_volatile(')!=1 or 'memory_region_symbol!' in entry:raise ValueError('new native store/alias path')
    if entry.index('state._transfer = Some(')>entry.index('once.begin(setup, endpoints, command, pool, io)'):raise ValueError('unretained transfer')
    for token in ['mod pixel_ipa_transfer;','pixel_ipa_transfer::start(&d, &mut state)',
        'pixel_ipa_transfer::late_notification(self)',
        '_transfer: Option<personal_os_rust_server::sunfish_ipa_linux_transfer::TransferOnce>']:
        if token not in parent:raise ValueError('parent transfer binding changed: '+token)
    for token in ['#![forbid(unsafe_code)]','pool.claim_memory_setup()', 'self.started_at = io.ticks()',
        'self.descriptors(pool, io, true)?','self.descriptors(pool, io, false)?',
        'self.report.published = true','self.plan.completion(pointer, bytes)',
        'self.canaries(pool, io)?','self.targets(pool, io, false)?','self.report.ack_sent = true',
        'pool.finish_memory_setup()','pool.quarantine()', 'now.wrapping_sub(self.started_at)',
        'pub fn late_fault','self.report.phase = Phase::Failed']:
        if re.sub(r'\s+','',token) not in re.sub(r'\s+','',core):raise ValueError('transfer coordinator changed: '+token)
    for token in ['self.memory_setup_claimed','self.memory_setup_complete','if !self.prepared',
        'Kind::CommandPayload','Kind::Tre(1)','4 * COMMAND_ELEMENT_BYTES','4 * RING_ELEMENT_BYTES','|| !permitted']:
        if token not in pool:raise ValueError('pool boundary changed: '+token)
    memory_writer=pool.split('pub(crate) fn memory_setup_write32(',1)[1].split('pub(crate) fn finish_memory_setup(',1)[0]
    if '|| !permitted' not in memory_writer:raise ValueError('memory pool range gate removed')

def audit():
    paths=['rust/src/pixel_ipa_transfer.rs','rust/src/sunfish_native_startup_preflight.rs',
        'rust/src/sunfish_ipa_linux_transfer.rs','rust/src/sunfish_ipa_linux_buffers.rs']
    texts=[(ROOT/p).read_text() for p in paths];check(*texts)
    mutations=[(0,'if !allowed(r.register(), r.value())','if false'),
        (0,'uc::Io::guard(&mut retained)','Ok::<(), uc::Error>(())'),
        (0,'state._transfer = Some(','let unretained = Some('),
        (0,'if badge != 1 << pixel_ipa_gsi::CHANNEL','if false'),
        (2,'self.targets(pool, io, false)?','Ok::<(), Error>(())?'),
        (2,'pool.claim_memory_setup()','Ok::<(), buffers::MemoryError>(())'),
        (3,'|| !permitted','|| false')]
    for i,old,new in mutations:
        assert texts[i].count(old)==(2 if old=='|| !permitted' else 1),(i,old)
        mutant=texts.copy();mutant[i]=mutant[i].replace(old,new,1)
        try:check(*mutant)
        except ValueError:continue
        raise ValueError('native transfer mutation accepted: '+old)
    pure='fn allowed('+texts[0].split('fn allowed(',1)[1].split('fn counter()',1)[0]
    names=['sunfish_ipa_linux_buffers','sunfish_ipa_linux_config','sunfish_ipa_linux_memory',
        'sunfish_ipa_linux_memsetup','sunfish_ipa_linux_gsi','sunfish_ipa_linux_endpoint',
        'sunfish_ipa_linux_command','sunfish_ipa_linux_tables','sunfish_ipa_linux_transfer','sunfish_ipa_linux_table_transfer']
    harness='\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names)
    harness+='\nuse sunfish_ipa_linux_transfer as dma;\n'+pure+r'''
#[test]
fn actual_native_transfer_whitelist_matches_compiled_c_and_rejects_other_words() {
 let regs=[dma::Register::ChannelDoorbell,dma::Register::EventDoorbell,dma::Register::IeobClear,
  dma::Register::IeobMask,dma::Register::TypeMask,dma::Register::LocalContext];
 for line in std::fs::read_to_string(std::env::var("DMA_C").unwrap()).unwrap().lines(){
  let p:Vec<_>=line.split_whitespace().collect();let o=usize::from_str_radix(p[0],16).unwrap();let v=u32::from_str_radix(p[1],16).unwrap();
  let r=*regs.iter().find(|r|r.offset()==o).unwrap();assert!(allowed(r,v));
 }
 for r in regs {for v in (0..=65535).chain([0x80004040,0x80003000,0x80004030,0x80004050,0x80003010,u32::MAX]){
  let expected=match r {dma::Register::ChannelDoorbell=>v==0x80004040,dma::Register::EventDoorbell=>v==0x80003000,
   dma::Register::IeobClear=>v==1,dma::Register::IeobMask=>v<=1,dma::Register::TypeMask=>[0,0x44,0x4c].contains(&v),dma::Register::LocalContext=>v==0x5f0};
  assert_eq!(allowed(r,v),expected);
 }}
}
'''
    p=OUT/'native-whitelist.rs';p.write_text(harness);binary=OUT/'native-whitelist'
    env=dict(os.environ,DMA_C=str(OUT/'linux-transfer-writes.txt'))
    for label,args in [('native-whitelist-build',['rustc','--edition=2024','-D','warnings','--test',str(p),'-o',str(binary)]),('native-whitelist',[str(binary)])]:
        r=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True);(OUT/(label+'.log')).write_text(r.stdout+r.stderr)
        if r.returncode:raise ValueError(label+' failed; see private log')
    spec=importlib.util.spec_from_file_location('transfer_command_audit',HERE/'audit-native-ipa-command-source.py')
    base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base);result=base.audit()
    result['native_transfer']={'source_sha256':dict(zip(paths,[hashlib.sha256(t.encode()).hexdigest()for t in texts])),
        'mutations_rejected':len(mutations),'compiled_native_whitelist':True,'channel':1,'event':0,'transactions':1,
        'timeout_seconds':1,'counter':'cntpct_el0','startup_irq':464,'microkit_channel':3,
        'scope':'One fixed IPA memory transaction and direct processing-context write; no MPSS/WLAN or flash',
        'functional_dma_proven':False,'production_ready':False}
    return result
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.write_text(json.dumps(audit(),indent=2)+'\n');print('Native first-transfer owner, write, IRQ and counter bindings pass; 7 new mutations rejected.')
