#!/usr/bin/env python3
"""Bind pre-auth SMEM execution to real retained owners and injected views."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,re
import sys
ROOT=Path(__file__).resolve().parents[2];HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE))
from proof_source_compat import source_matches_compatible
PATHS=['rust/src/pixel_mpss_smem.rs','rust/src/pixel_smmu_install.rs',
       'rust/src/pixel_linux_rmtfs.rs','rust/src/sunfish_native_startup_preflight.rs','rust/Cargo.toml',
       'rust/src/sunfish_smmu_native.rs']
def compact(s):
    s=re.sub(r'/\*.*?\*/','',s,flags=re.S);s=re.sub(r'//[^\n]*','',s)
    return ''.join(s.split())
def check(stage,domain,factory,entry,cargo,native):
    p,d,f,e,c=map(compact,(stage,domain,factory,entry,cargo))
    for token in ['STATE.compare_exchange(0,1,Ordering::AcqRel,Ordering::Acquire)',
        'STATE.load(Ordering::Acquire)!=1','!pixel_linux_rmtfs::preboot_generation(generation)',
        '!pixel_smmu_install::preboot_faults_clear()',
        'pixel_ipa_receive::healthy(state)','d.retained_quiet()',
        'f.phase()==pixel_linux_rmtfs::Phase::Prepared','STATE.store(3,Ordering::Release)',
        '.is_none_or(|f|f.phase()!=pixel_linux_rmtfs::Phase::Prepared)',
        'iff.phase()!=pixel_linux_rmtfs::Phase::Failed{f.fault(code);}',
        '.attach_modem_smem(GENERATION,live)', 'for_in0..32','pixel_ipa_receive::poll_pending(state)',
        '.modem_smem_step()', '.take_modem_smem_prepared()', 'p.generation()==GENERATION',
        'state._modem_smem_prepared=Some(p)','STATE.store(2,Ordering::Release)']:
        if token not in p:raise ValueError('modem entry lifetime: '+token)
    for token in ['Window::new_with_modem_pages(',
        'memory_region_symbol!(mpss_smem_payload:*mutu32).as_ptr()',
        'read_volatile(&rawconstmpss_smem_payload_paddr)',
        'read_volatile(&rawconstsmmu_gr0)as*constu32',
        'read_volatile(&rawconstsmmu_context)as*constu32',
        'gr0asusize!=0x70000000||cbasusize!=0x70002000',
        'read_volatile(&rawconstsmmu_gr0_paddr)!=0x15000000',
        'read_volatile(&rawconstsmmu_context_paddr)!=0x15084000',
        'gr0.add(0x48/4).read_volatile()', 'cb.add(0x58/4).read_volatile()',
        'global==0&&context&!(3<<9)==0', 'ifFAULT.load(Ordering::Acquire){returnfalse;}',
        'if!self.retained_quiet(){returnErr(Error::State);}']:
        if token not in d:raise ValueError('modem delegated domain binding: '+token)
    for token in ['generation==GENERATION&&COLD.load(Ordering::Acquire)==1&&mappings().is_ok()']:
        if token not in f:raise ValueError('modem cold factory lifecycle: '+token)
    if 'sunfish-native-startup-mpss-smem=["sunfish-native-startup-linux-rmtfs"]'not in c:raise ValueError('feature dependency')
    for token in ['modpixel_mpss_smem;','_modem_smem_prepared:Option<personal_os_rust_server::sunfish_mpss_linux_smem::Prepared>',
                  'pixel_mpss_smem::prepare(&d,&mutstate)','pixel_mpss_smem::fault(self,0x20)',
                  'pixel_mpss_smem::show(&unsafe{Display::new(registers.pixels)},self)']:
        if token not in e:raise ValueError('missing boot hook: '+token)
    if e.index('prepare_linux_rmtfs(&d,&mutstate)')>e.index('pixel_mpss_smem::prepare(&d,&mutstate)'):raise ValueError('extension before providers')
    for token in ['seL4_ARM_SMC','pas_auth','notify_peer','write_volatile','copy_nonoverlapping','transmute','qcom_glink','qcom_msa']:
        if token in p:raise ValueError('unexpected operation in pre-auth stage: '+token)
    n=compact(native)
    if ('pubfnfault_status(&self)->(u32,u32){(self.global_fault_status(),self.context_fault_status())}' not in n
        or 'pubfnglobal_fault_status(&self)->u32{self.load32(0,0x48)}' not in n
        or 'pubfncontext_fault_status(&self)->u32{self.load32(2,0x58)}' not in n):
        raise ValueError('re-audit split global/context FSR offsets and pair delegation')
    body=n[n.index('fnwith_retained_modem<T>('):n.index('pubfnattach_modem_smem(')]
    if body.count('self.retained_receive_domain()?;')!=2 or 'self.check()'in body or 'self.failed='in body:
        raise ValueError('post-install operations must preserve the consumed install path and revalidate twice')
    for token in ['ifself.modem_quarantined{returnErr(Error::State);}',
                  'self.modem_quarantined=true;','self.modem_quarantined=false;',
                  'action(self.shared_memory.as_mut().ok_or(Error::State)?)']:
        if token not in body:raise ValueError('missing post-install quarantine: '+token)
    before=body.index('self.modem_quarantined=true;');first=body.index('self.retained_receive_domain()?;')
    action=body.index('action(');last=body.rindex('self.retained_receive_domain()?;');clear=body.index('self.modem_quarantined=false;')
    if not before<first<action<last<clear:raise ValueError('post-install operation ordering')
    if n.count('self.with_retained_modem(')!=7:raise ValueError('six modem operations and the opt-in GLINK forwarder must use retained-domain checks')
    if 'self.with_retained_modem(|memory|memory.glink(action))'not in n:
        raise ValueError('optional GLINK must use the same retained modem owner')

def audit():
    s=importlib.util.spec_from_file_location('mpss_rmtfs_source',HERE/'audit-native-linux-rmtfs-source.py')
    base=importlib.util.module_from_spec(s);s.loader.exec_module(base);previous=base.audit()
    texts=[(ROOT/p).read_text()for p in PATHS];check(*texts)
    mutations=[
        (0,'!pixel_smmu_install::preboot_faults_clear()','false'),
        (0,'STATE.load(Ordering::Acquire) != 1','false'),
        (0,'f.phase() == pixel_linux_rmtfs::Phase::Prepared','true'),
        (0,'pixel_ipa_receive::poll_pending(state);','/* ignore delivery */'),
        (0,'p.generation() == GENERATION','true'),
        (1,'Window::new_with_modem_pages(','Window::new('),
        (1,'core::ptr::read_volatile(&raw const mpss_smem_payload_paddr)','0x8611f000'),
        (1,'global == 0 && context & !(3 << 9) == 0','true'),
        (2,'generation == GENERATION && COLD.load(Ordering::Acquire) == 1 && mappings().is_ok()','true'),
        (3,'pixel_mpss_smem::fault(self, 0x20);','/* ignore late fault */'),
    ]
    for i,before,after in mutations:
        if not texts[i].count(before):raise ValueError('source mutation missing: '+before)
        changed=list(texts);changed[i]=changed[i].replace(before,after)
        try:check(*changed)
        except ValueError:pass
        else:raise ValueError('source mutation accepted: '+before)
    proof=ROOT/'.cache/sunfish-native-wifi/private/mpss-linux-smem/result.json';p=json.loads(proof.read_text())
    for name,h in p['source_sha256'].items():
        if not source_matches_compatible(name,h):raise ValueError('stale extension proof: '+name)
    entry_proof=ROOT/'.cache/sunfish-native-wifi/private/mpss-smem-entry/result.json'
    j=json.loads(entry_proof.read_text())
    if j['result']!='passed' or len(j['cases'])!=11 or not j['fresh_process_per_case'] or j['device_actions']:
        raise ValueError('invalid real entry host proof')
    for name,h in j['source_sha256'].items():
        if not source_matches_compatible(name,h):raise ValueError('stale actual entry proof: '+name)
    postinstall=ROOT/'.cache/sunfish-native-wifi/private/mpss-postinstall/result.json'
    k=json.loads(postinstall.read_text())
    if k['result']!='passed' or k['postinstall_groups']!=4 or len(k['mutations_rejected'])!=5 or k['device_actions']:
        raise ValueError('missing actual post-install adapter regression')
    for name,h in k['source_sha256'].items():
        if not source_matches_compatible(name,h):raise ValueError('stale post-install proof: '+name)
    return {'scope':'Concrete pre-auth retained SMEM boot-entry composition; no new SMC, IRQ, peer notification or modem/WLAN start.',
        'source_sha256':{p:hashlib.sha256(t.encode()).hexdigest()for p,t in zip(PATHS,texts)},
        'source_mutations_rejected':len(mutations),'predecessor_source_audit':previous,
        'extension_proof_sha256':hashlib.sha256(proof.read_bytes()).hexdigest(),
        'entry_host_proof_sha256':hashlib.sha256(entry_proof.read_bytes()).hexdigest(),
        'postinstall_native_proof_sha256':hashlib.sha256(postinstall.read_bytes()).hexdigest(),
        'postinstall_behavioral_groups':4,'postinstall_mutations_rejected':5,
        'entry_host_cases':len(j['cases']),
        'physical_modem_smem_proven':False,'native_mpss_started':False,'ready_for_device_test':False}
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.write_text(json.dumps(audit(),indent=2)+'\n');print('PASS: concrete retained modem-memory boot entry and ten rejected source mutations. No device action.')
