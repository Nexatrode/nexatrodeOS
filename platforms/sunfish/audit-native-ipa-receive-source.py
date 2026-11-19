#!/usr/bin/env python3
"""Source binding of native receive entry, sole ownership, MMIO policy and IRQ dispatch."""
from pathlib import Path
import argparse,hashlib,importlib.util,json
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[1]
s=importlib.util.spec_from_file_location('native_receive_memory_source',HERE/'audit-native-ipa-receive-memory-source.py')
base=importlib.util.module_from_spec(s);s.loader.exec_module(base)
def compact(s):return ''.join(s.split())
def check(entry,native,cargo,command):
    if 'sunfish-native-startup-ipa-receive = ["sunfish-native-startup-ipa-receive-memory"]'not in cargo:raise ValueError('receive feature chain')
    e=compact(entry);n=compact(native);q=compact(command)
    for token in ['modpixel_ipa_receive;','_receive_once:Option<pixel_ipa_receive::Once>',
        '_receive_runtime:Option<pixel_ipa_receive::Runtime>','pixel_ipa_receive::start(&d,&mutstate);',
        'ifpixel_ipa_receive::notified(self,channels){returnOk(());}']:
        if token not in e:raise ValueError('native receive entry: '+token)
    if e.index('pixel_ipa_receive_memory::prepare(')>e.index('pixel_ipa_receive::start('):raise ValueError('receive before RAM preparation')
    if e.index('pixel_ipa_receive::notified(self,channels)')>e.index('pixel_ipa_tables::late_notification(self)'):raise ValueError('legacy IRQ handler precedes receive owner')
    for token in ['if!policy::startup(w)','if!policy::receive(r.register(),r.value())',
        'retained.retained_prerequisites()','policy::config(self.expected,actual,state)',
        'policy::rx_state(state,observed)','!pool.table_setup_complete()','!data.prepared()',
        '!self.firmware.pas_report().passed()','self.scm.report().error.is_some()',
        'channel_words.into_iter().take(5).chain(event_words.into_iter().take(5))',
        'state._buffers.take().unwrap()','state._receive_data.take().unwrap()',
        'state._receive_once=Some(Once::new(rings,data,hz))',
        'Runtime::new(runtime::Runtime::new(once),uc)',
        'state._receive_once.as_ref()?.pools().0',
        'Channel::new(pixel_ipa_gsi::CHANNEL).irq_ack()',
        'Channel::new(irq::CHANNELasusize).irq_ack()',
        'registers.shared.add(f.offset()).read_volatile()',
        'packet.read_into(at,&mutbytes[..n])','r.notification(notice,i,s)',
        'ifstate._receive_stats.terminal{return;}']:
        if token not in n:raise ValueError('native receive binding: '+token)
    if n.index('state._receive_once=Some(Once::new(rings,data,hz))')>n.index('.begin('):raise ValueError('receiver not retained before START')
    if 'fnacknowledge_start(&mutself)->Result<(),start::Error>{Channel::new(pixel_ipa_gsi::CHANNEL).irq_ack().map_err(|_|start::Error::Acknowledge)?;'not in n:
        raise ValueError('START ACK bound to wrong physical channel')
    for token in ['memory_region_symbol!','NativeRam::new','NativeReceiveRam::new','copy_nonoverlapping','call_smc','transmute']:
        if token in native:raise ValueError('unexpected native operation: '+token)
    if 'ifpixel_ipa_receive::owns(state){pixel_ipa_receive::quarantine(state);return;}'not in q:raise ValueError('legacy containment can replay receiver masks')
def audit():
    result=base.audit()
    paths=['rust/src/sunfish_native_startup_preflight.rs','rust/src/pixel_ipa_receive.rs','rust/Cargo.toml','rust/src/pixel_ipa_command.rs']
    texts=[(ROOT/p).read_text()for p in paths];check(*texts)
    changes=[(0,'pixel_ipa_receive::start(&d, &mut state);','/* no receiver */'),
        (1,'if !policy::startup(w)','if false'),(1,'if !policy::receive(r.register(), r.value())','if false'),
        (1,'!self.firmware.pas_report().passed()','false'),
        (1,'Channel::new(pixel_ipa_gsi::CHANNEL)\n            .irq_ack()\n            .map_err(|_| start::Error::Acknowledge)?;','Channel::new(2)\n            .irq_ack()\n            .map_err(|_| start::Error::Acknowledge)?;'),
        (1,'state._receive_once = Some(Once::new(rings, data, hz));','/* missing retained receiver */')]
    for i,before,after in changes:
        if texts[i].count(before)!=1:raise ValueError('source mutation not unique: '+before)
        changed=list(texts);changed[i]=changed[i].replace(before,after)
        try:check(*changed)
        except ValueError:pass
        else:raise ValueError('native source mutation accepted: '+before)
    proof=ROOT/'.cache/sunfish-native-wifi/private/ipa-receive-native-policy/manifest.json';j=json.loads(proof.read_text())
    for name,h in j['source_sha256'].items():
        if hashlib.sha256((ROOT/name).read_bytes()).hexdigest()!=h:raise ValueError('Stale native policy proof: '+name)
    result['native_receive']={'source_sha256':{p:hashlib.sha256(t.encode()).hexdigest()for p,t in zip(paths,texts)},
        'policy_proof_sha256':hashlib.sha256(proof.read_bytes()).hexdigest(),'source_mutations_rejected':len(changes),
        'receive_start':True,'initial_buffers':251,'initial_doorbells':16,'shared_ipa_gsi_dispatch':True,
        'diagnostic_aggregate_reads_only':True,'network_delivery':False,'modem_or_wlan_start':False,'physical_receive_proven':False}
    return result
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.write_text(json.dumps(audit(),indent=2)+'\n');print('PASS native receive source binding and six rejected mutations; no device action.')
