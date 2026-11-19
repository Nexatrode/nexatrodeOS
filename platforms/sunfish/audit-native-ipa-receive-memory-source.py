#!/usr/bin/env python3
"""Bind the concrete receive-memory entry to retained owners, no RX start."""
from pathlib import Path
import argparse,hashlib,importlib.util,json
HERE=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('receive_memory_previous_source',HERE/'audit-native-ipa-tables-source.py')
previous=importlib.util.module_from_spec(s);s.loader.exec_module(previous)
ROOT=HERE.parents[1]

def check(entry,smmu,memory,cargo,command):
    for token in ['mod pixel_ipa_receive_memory;','_receive_data: Option<',
                  'pixel_ipa_receive_memory::prepare(&d, &mut state);','data.quarantine();']:
        if token not in entry:raise ValueError('receive entry binding: '+token)
    if entry.index('pixel_ipa_tables::start(&d, &mut state);')>entry.index('pixel_ipa_receive_memory::prepare(&d, &mut state);'):
        raise ValueError('receive preparation before real table completion')
    if 'sunfish-native-startup-ipa-receive-memory = ["sunfish-native-startup-ipa-tables"]' not in cargo:
        raise ValueError('receive memory feature dependencies')
    for token in ['memory_region_symbol!(smmu_l3_receive:', 'memory_region_symbol!(ipa_receive_data:',
                  'read_volatile(&raw const ipa_receive_data_paddr)','read_volatile(&raw const smmu_l3_receive_paddr)',
                  'NativeReceiveRam::new(receive_data, &physical)', 'adapter.attach_receive_memory(ram)?;',
                  'adapter.receive_mappings()?;', 'adapter.stage_receive_root_tables(',
                  'plan_linux_reference_receive_root_install(',
                  '.take_receive_memory()']:
        if token not in smmu:raise ValueError('receive SMMU binding: '+token)
    if 'adapter.retained_receive_domain_with(&mutprogress).is_ok()' not in ''.join(smmu.split()):
        raise ValueError('receive SMMU retained-domain audit result is not checked')
    for token in ['state._domain.as_mut()', 'p.take_receive_memory().ok()', 'DataPool::new(memory)',
                  'state._receive_data = Some(pool);','data.prepare(|| guard.check())',
                  'data.verify_guards(|| guard.check())','uc::Io::guard(&mut self.irq)',
                  '!p.table_setup_complete()', 'self.tables.report().target_words != 120',
                  'self.memory.report().target_words != 1619','cmd::Read::IeobMask, 1',
                  'cmd::Read::TypeMask, 0x4c','cmd::Read::IeobStatus, 0',
                  'pixel_ipa_command::quarantine(state)', 'NO RX CHANNEL START','NO RECEIVE DOORBELL']:
        if ''.join(token.split()) not in ''.join(memory.split()):raise ValueError('receive concrete guard: '+token)
    if memory.index('state._receive_data = Some(pool);')>memory.index('data.prepare(|| guard.check())'):
        raise ValueError('CPU receive stores before owner retention')
    for token in ['write_volatile','write_tre','0x1f008','0x1e010','0x1e108','call_smc','copy_nonoverlapping']:
        if token in memory:raise ValueError('unrequested receive operation: '+token)
    if 'state._receive_data.as_mut()' not in command or 'data.quarantine();' not in command:
        raise ValueError('receive pool omitted from inherited failure containment')

def audit():
    result=previous.audit()
    paths=['rust/src/sunfish_native_startup_preflight.rs','rust/src/pixel_smmu_install.rs',
        'rust/src/pixel_ipa_receive_memory.rs','rust/Cargo.toml','rust/src/pixel_ipa_command.rs']
    texts=[(ROOT/n).read_text()for n in paths];check(*texts)
    mutations=[(0,'pixel_ipa_receive_memory::prepare(&d, &mut state);','/* missing receive preparation */'),
        (1,'adapter.attach_receive_memory(ram)?;','/* no real RAM retained */'),
        (1,'read_volatile(&raw const ipa_receive_data_paddr)','read_volatile(&raw const smmu_data_paddr)'),
        (2,'state._receive_data = Some(pool);','/* CPU stores without owner */'),
        (2,'self.tables.report().target_words != 120','false'),
        (2,'cmd::Read::IeobStatus, 0','cmd::Read::IeobStatus, 1'),
        (2,'data.verify_guards(|| guard.check())','data.verify_guards(|| Ok(()))'),
        (4,'data.quarantine();','/* missing receive quarantine */')]
    for index,before,after in mutations:
        if texts[index].count(before)!=1:raise ValueError('receive mutation not unique: '+before)
        changed=list(texts);changed[index]=changed[index].replace(before,after)
        try:check(*changed)
        except ValueError:pass
        else:raise ValueError('receive source mutation accepted: '+before)
    result['receive_memory_entry']={'source_sha256':{p:hashlib.sha256(t.encode()).hexdigest()for p,t in zip(paths,texts)},
        'source_mutations_rejected':len(mutations),'table_frames':8,'receive_cpu_pages':512,'receive_dma_pages':502,
        'cpu_tail_pages':10,'cpu_preparation_only':True,'receive_channel_start':False,'receive_doorbell':False,
        'packet_access':False,'modem_or_wlan_start':False,'physical_allocation_proven':False}
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.write_text(json.dumps(audit(),indent=2)+'\n')
    print('Receive-memory entry bound to real retained owners; eight source mutations rejected; no RX start.')
