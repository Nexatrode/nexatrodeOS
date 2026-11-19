#!/usr/bin/env python3
"""Bind the concrete cold factory to the tested selected RMTFS/core policy."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import sys

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from proof_source_compat import source_matches_compatible
PATHS=['rust/src/pixel_linux_rmtfs.rs','rust/src/sunfish_native_startup_preflight.rs',
       'rust/src/pixel_ipa_receive.rs','rust/Cargo.toml']

def compact(text):
    text=re.sub(r'/\*.*?\*/','',text,flags=re.S)
    return ''.join(re.sub(r'//[^\n]*','',text).split())

def check(factory,entry,receive,cargo):
    f,e,r,c=map(compact,(factory,entry,receive,cargo))
    required=[
      'constBUFFER_VA:u64=0x77001000;','constSCRATCH_VA:u64=0x77301000;',
      'bp!=pool::BUFFER_PA','COLD.compare_exchange(0,1,Ordering::AcqRel,Ordering::Acquire).is_err()',
      'letphase=COLD.load(Ordering::Acquire);letallowed=phase==1;',
      '#[cfg(feature="sunfish-native-startup-mpss-auth")]letallowed=allowed||matches!(phase,3|4|5);',
      'if!allowed||mappings()!=Ok(self.maps)',
      'modem_stopped:phase==1,initial_hlos:phase==1,',
      'pub(super)unsafefnbegin(&mutself)',
      'ifc.words()!=expected.words()||c.bytes()!=expected.bytes()',
      'seL4_ARM_SMC_Call(7,&args,&mutr)','if e!=0'.replace(' ',''),
      'scm::invoke_checked(&mutMonitor,&mutDelay,&mutCheck(self.maps),*c.words())',
      'self.resources=Some(Resources::attach(p,self.storage.take().unwrap()))',
      'self.resources.as_mut().unwrap().prepare_step()',
      'self.resources.as_mut().unwrap().share().is_err()',
      'self.resources.take().unwrap()',
      's.route_binding()!=(1,0,0x5000)','s.next_transfer_port()!=0x5100',
      's.enable_linux_ipa(GENERATION,0x4004)?','s.prepare_tftp_server(GENERATION,0x5000)?',
      'COLD.store(2,Ordering::Release);self.phase=Phase::Failed',
      'Phase::New|Phase::Prepared|Phase::Failed=>{}',
    ]
    for token in required:
        if token not in f:raise ValueError('cold factory binding: '+token)
    for token in ['.start_control(','.queue_reply(','.ingest(','qcom_pas_','qcom_msa','qcom_wlfw','copy_nonoverlapping','transmute']:
        if token in f:raise ValueError('unexpected cold factory authority: '+token)
    if f.count('seL4_ARM_SMC_Call(')!=1:raise ValueError('multiple raw SMC paths')
    if f.index('self.resources=Some(Resources::attach(')>f.index('self.resources.as_mut().unwrap().prepare_step()'):
        raise ValueError('resources not retained before preparation')
    for token in ['_linux_rmtfs:Option<pixel_linux_rmtfs::Factory>',
        'state._linux_rmtfs=Some(pixel_linux_rmtfs::Factory::new(storage,tftp))',
        'if!pixel_ipa_receive::healthy(state)||state._linux_rmtfs.is_some()',
        'ifstate._storage.is_none()||state._tftp.is_none()',
        'unsafe{state._linux_rmtfs.as_mut().unwrap().begin();}',
        'forstepin0..514','pixel_ipa_receive::poll_pending(state)',
        'self._linux_rmtfs.as_mut().unwrap().fault(0x20)',
        'not(feature="sunfish-native-startup-linux-rmtfs")']:
        if token not in e:raise ValueError('native factory entry: '+token)
    if e.index('pixel_ipa_receive::start(&d,&mutstate)')>e.index('prepare_linux_rmtfs(&d,&mutstate)'):
        raise ValueError('RMTFS before retained receive startup')
    for token in ['!state._receive_stats.terminal','r.error().is_none()','r.startup_report().phase==start::Phase::Armed']:
        if token not in r:raise ValueError('lost IPA lifecycle condition: '+token)
    if 'sunfish-native-startup-linux-rmtfs=["sunfish-native-startup-ipa-receive","sunfish-linux-rmtfs"]'not in c:
        raise ValueError('wrong explicit opt-in feature chain')

def audit():
    texts=[(ROOT/p).read_text()for p in PATHS];check(*texts)
    mutants=[
        (0,'seL4_ARM_SMC_Call(7,','seL4_ARM_SMC_Call(8,'),
        (0,'pub(super) unsafe fn begin','pub(super) fn begin'),
        (0,'if c.words() != expected.words() || c.bytes() != expected.bytes()','if false'),
        (0,'if !allowed || mappings() != Ok(self.maps)','if false'),
        (0,'s.next_transfer_port() != 0x5100','false'),
        (1,'if !pixel_ipa_receive::healthy(state) || state._linux_rmtfs.is_some()','if false'),
    ]
    for index,before,after in mutants:
        if texts[index].count(before)!=1:raise ValueError('nonunique factory mutation: '+before)
        changed=list(texts);changed[index]=changed[index].replace(before,after)
        try:check(*changed)
        except ValueError:pass
        else:raise ValueError('factory source mutation accepted: '+before)
    proof=ROOT/'.cache/sunfish-native-wifi/private/linux-rmtfs-server/manifest.json'
    m=json.loads(proof.read_text())
    for name,digest in m['source_sha256'].items():
        if not source_matches_compatible(name,digest):raise ValueError('stale storage/owner proof: '+name)
    return {'scope':'Concrete seL4 mapping/SCM/preboot-provider source composition. The host cold/device contract is an unsafe precondition; neither source tests nor local flags prove it. No MPSS startup.',
        'source_sha256':{p:hashlib.sha256(t.encode()).hexdigest()for p,t in zip(PATHS,texts)},
        'storage_owner_proof_sha256':hashlib.sha256(proof.read_bytes()).hexdigest(),
        'source_mutations_rejected':len(mutants),'cold_contract_required':True,'cold_contract_software_proven':False,
        'mpss_start':False,'physical_rmtfs_assignment_proven':False,'production_ready':False}

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.write_text(json.dumps(audit(),indent=2)+'\n');print('PASS: concrete cold RMTFS factory source and six rejected source mutations; no device action.')
