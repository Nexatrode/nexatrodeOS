#!/usr/bin/env python3
"""Bind the native endpoint/channel-1 START path and compile its whitelists."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,re,subprocess
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-command'
def check(entry,parent,core):
    required=['uc::Io::guard(&mut self.retained)','self.setup.report().phase != gsi::SetupPhase::Complete',
        'for w in ep::PROFILE','r.readback != ep::COMPLETE','state._endpoint = Some(ep::ConfigureOnce::new())',
        'state._command = Some(', 'once.begin(setup, endpoints, io)', 'once.notified(io, now)',
        'once.timeout(io, now)', 'once.external_fault(io)', 'if badge != 1 << pixel_ipa_gsi::CHANNEL',
        'let badge = receive();','if badge != 0', 'Channel::new(pixel_ipa_gsi::CHANNEL)',
        'sel4::Cap::<sel4::cap_type::Endpoint>::from_bits(1)','sel4::Cap::<sel4::cap_type::Reply>::from_bits(4)',
        '.nb_recv(', 'cntpct_el0','cntfrq_el0','1_000_000..=1_000_000_000',
        'if !endpoint_word_allowed(w)', 'if !command_word_allowed(r.register(), r.value())',
        'cmd::Register::Command => value == cmd::START', 'ep::PROFILE.contains(&word)',
        'pool.quarantine()', 'domain.stop_on_fault()', 'dispatch_uc(state, true)',
        'self.retained.registers.gsi,', '.write_volatile(value.to_le())',
        'r.gsi.cast_mut().add(gsi::TYPE_MASK / 4).write_volatile(0)',
        'if word(r.gsi, gsi::TYPE_MASK) != 0']
    for token in required:
        if token not in entry:raise ValueError('native command path changed: '+token)
    if not re.search(r'self\.store\(\s*self\.retained\.registers\.ipa,\s*w\.offset\(\),\s*w\.value,?\s*\)',entry):
        raise ValueError('native endpoint store binding changed')
    if entry.index('state._endpoint = Some(')>entry.index('endpoints(state)'):raise ValueError('endpoint owner not retained')
    if entry.index('state._command = Some(')>entry.index('once.begin(setup, endpoints, io)'):raise ValueError('command owner not retained')
    if entry.count('.write_volatile(')!=2 or entry.count('self.store(')!=3:raise ValueError('unreviewed native store path')
    for token in ['mod pixel_ipa_command;', 'pixel_ipa_command::start(&d, &mut state)',
        'pixel_ipa_command::late_notification(self)', '_command: Option<personal_os_rust_server::sunfish_ipa_linux_command::StartOnce>']:
        if token not in parent:raise ValueError('native command integration changed: '+token)
    for token in ['START: u32 = 0x01000001','self.started_at = io.ticks()', 'if kind != 1 || source != 2',
        'self.quiet_sources(io)?', 'self.states(io, 2)?','self.report.ack_sent = true']:
        if token not in core:raise ValueError('command core binding changed: '+token)
def audit():
    paths=['rust/src/pixel_ipa_command.rs','rust/src/sunfish_native_startup_preflight.rs','rust/src/sunfish_ipa_linux_command.rs']
    texts=[(ROOT/p).read_text()for p in paths];check(*texts)
    mutations=[(0,'value == cmd::START','value != 0'),(0,'uc::Io::guard(&mut self.retained)','Ok::<(), uc::Error>(())'),
        (0,'once.timeout(io, now)','Ok(())'),(0,'state._command = Some(','let unretained = Some('),
        (0,'if badge != 1 << pixel_ipa_gsi::CHANNEL','if false'),
        (2,'if kind != 1 || source != 2','if kind != 1'),
        (0,'ep::PROFILE.contains(&word)','true')]
    for i,old,new in mutations:
        assert texts[i].count(old)==1,old
        v=texts.copy();v[i]=v[i].replace(old,new)
        try:check(*v)
        except ValueError:continue
        raise ValueError('native command mutation accepted: '+old)
    pure='fn endpoint_word_allowed'+texts[0].split('fn endpoint_word_allowed',1)[1].split('fn counter()',1)[0]
    names=['sunfish_ipa_linux_buffers','sunfish_ipa_linux_gsi','sunfish_ipa_linux_endpoint','sunfish_ipa_linux_command']
    fixture='\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names)
    fixture+='\nuse sunfish_ipa_linux_endpoint as ep;\nuse sunfish_ipa_linux_command as cmd;\n'+pure+r'''
#[test]
fn native_command_whitelists_match_linux_and_reject_foreign_values(){
 for line in std::fs::read_to_string(std::env::var("EP_C").unwrap()).unwrap().lines(){
  let p:Vec<_>=line.split_whitespace().collect();let o=usize::from_str_radix(p[0],16).unwrap();let v=u32::from_str_radix(p[1],16).unwrap();
  let w=*ep::PROFILE.iter().find(|w|w.offset()==o).unwrap();assert_eq!(w.value,v);assert!(endpoint_word_allowed(w));
  assert!(!endpoint_word_allowed(ep::Word{value:v^1,..w}));
 }
 let regs=[cmd::Register::TypeMask,cmd::Register::ChannelMask,cmd::Register::ChannelClear,cmd::Register::Command,cmd::Register::IeobMask];
 for line in std::fs::read_to_string(std::env::var("CMD_C").unwrap()).unwrap().lines(){
  let p:Vec<_>=line.split_whitespace().collect();let o=usize::from_str_radix(p[0],16).unwrap();let v=u32::from_str_radix(p[1],16).unwrap();
  let r=*regs.iter().find(|r|r.offset()==o).unwrap();assert!(command_word_allowed(r,v));
 }
 for opcode in 0..=255 {for id in 0..32 {
  let v=opcode<<24|id;assert_eq!(command_word_allowed(cmd::Register::Command,v),v==0x01000001);
 }}
 for r in regs {for v in [0,1,2,3,4,0x44,0x4c,0x4d,0x01000000,0x01000002,u32::MAX]{
  let expected=match r {cmd::Register::TypeMask=>[0,0x4c,0x4d].contains(&v),cmd::Register::ChannelMask=>[0,2].contains(&v),
   cmd::Register::ChannelClear=>[2,u32::MAX].contains(&v),cmd::Register::Command=>v==0x01000001,cmd::Register::IeobMask=>v==1};
  assert_eq!(command_word_allowed(r,v),expected);
 }}
}
'''
    p=OUT/'native-whitelist.rs';p.write_text(fixture)
    env=dict(os.environ,EP_C=str(OUT.parent/'ipa-linux-endpoint/linux-endpoint-writes.txt'),CMD_C=str(OUT/'linux-command-writes.txt'))
    binary=OUT/'native-whitelist'
    for label,args in [('native-whitelist-build',['rustc','--edition=2024','-D','warnings','--test',str(p),'-o',str(binary)]),('native-whitelist',[str(binary)])]:
        r=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True);(OUT/(label+'.log')).write_text(r.stdout+r.stderr)
        if r.returncode:raise ValueError(label+' failed; see private log')
    spec=importlib.util.spec_from_file_location('command_gsi_audit',HERE/'audit-native-ipa-gsi-source.py')
    base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base);result=base.audit()
    result['native_command']={'source_sha256':dict(zip(paths,[hashlib.sha256(t.encode()).hexdigest()for t in texts])),
        'mutations_rejected':len(mutations),'compiled_native_whitelists':True,'command_channel':1,'event_ring':0,
        'startup_irq':464,'microkit_channel':3,'counter':'cntpct_el0','timeout_ms':50,
        'scope':'Native endpoint configuration and empty command channel START; no TRE, transfer doorbell, MPSS/WLAN boot or flash',
        'physical_start_completion_proven':False,'functional_dma_proven':False}
    return result
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.write_text(json.dumps(audit(),indent=2)+'\n');print('Native command retained owners, closed writes, stale-badge gate and IRQ/counter path pass; 7 new mutations rejected.')
