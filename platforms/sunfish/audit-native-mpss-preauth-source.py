#!/usr/bin/env python3
"""Bind the combined pre-auth entry to its actual retained native providers."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,re
import sys
ROOT=Path(__file__).resolve().parents[2];HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE))
from proof_source_compat import source_matches_compatible
PATHS=['rust/src/pixel_mpss_preauth.rs','rust/src/pixel_smmu_install.rs','rust/src/sunfish_native_startup_preflight.rs','rust/Cargo.toml']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def compact(s):return ''.join(re.sub(r'//[^\n]*','',re.sub(r'/\*.*?\*/','',s,flags=re.S)).split())
def check(stage,domain,entry,cargo):
    p,d,e,c=map(compact,(stage,domain,entry,cargo))
    for token in ['STATE.compare_exchange(0,1,Ordering::AcqRel,Ordering::Acquire)',
                  's._modem_smem_prepared.is_none()', 'f.phase()!=pixel_linux_rmtfs::Phase::Prepared',
                  'pixel_ipa_receive::healthy(s)','d.retained_quiet()',
                  'pixel_linux_rmtfs::preboot_generation(g)','pixel_smmu_install::preboot_faults_clear()',
                  'memory_region_symbol!(mpss_aop_msgram:*mutu32).as_ptr()',
                  'memory_region_symbol!(mpss_apcs:*mutu32).as_ptr()',
                  '*var!(mpss_aop_msgram_paddr:usize=0)!=0x0c300000',
                  '*var!(mpss_apcs_paddr:usize=0)!=0x17c00000',
                  'ifviews()?!=self.views','self.database.check()',
                  'self.bytes[n*4..n*4+4]!=word(self.data,n*4).to_le_bytes()',
                  'self.pointer()!=(0x85f20000,DB_BYTESasu32)',
                  'native::Native::new(v.ram,0x0c300000,v.apcs,0x17c00000,65536,',
                  's._modem_power=Some(power)', 's._modem_power.as_mut().unwrap().enable(',
                  'signal::Native::new(v.apcs.add(3),0x17c0000c,',
                  'RAM.store(v.ramasusize,Ordering::Release)','APCS.store(v.apcsasusize,Ordering::Release)',
                  'v.ramasusize==RAM.load(Ordering::Acquire)','v.apcsasusize==APCS.load(Ordering::Acquire)',
                  '#[cfg(not(feature="sunfish-native-startup-mpss-auth"))]letlifecycle=smp::Lifecycle::PreAuth;',
                  '#[cfg(feature="sunfish-native-startup-mpss-auth")]letlifecycle=pixel_linux_rmtfs::lifecycle(g).ok_or(smp::Error::Guard)?;',
                  'lifecycle,',
                  'power::Resources::new(s._modem_power.as_mut().unwrap(),&s._scm_hold,s._firmware_rsc.as_mut().unwrap(),65536,)',
                  'resources.acquire()','resources.release(lease)',
                  'if!started(){return;}','iff.phase()!=pixel_linux_rmtfs::Phase::Failed',
                  'ERROR.compare_exchange(0,code,Ordering::AcqRel,Ordering::Acquire)',
                  'STATE.store(5,Ordering::Release)', 'STATE.store(4,Ordering::Release)']:
        if token not in p:raise ValueError('preauth retained binding: '+token)
    if p.count('ifverify(s).is_err()')!=2 or p.count('pixel_ipa_receive::poll_pending(s)')!=3:raise ValueError('missing both hold checks or IRQ checkpoint')
    retain=p.index('s._modem_power=Some(power)');enable=p.index('.enable(');v1=p.index('ifverify(s).is_err()');activate=p.index('.activate_modem_smp2p(');v2=p.rindex('ifverify(s).is_err()');final=p.index('STATE.store(3,Ordering::Release)')
    if not retain<enable<v1<activate<v2<final:raise ValueError('preauth lifetime/order/final publication')
    for token in ['seL4_ARM_SMC','write_volatile','qcom_glink','qcom_msa','sunfish_mpss_linux_loader','modem_smp2p_poll','transmute','copy_nonoverlapping']:
        if token in p:raise ValueError('unexpected preauth operation: '+token)
    for token in ['letretained=core::mem::ManuallyDrop::new(signal)','activate_modem_smp2p(prepared,core::mem::ManuallyDrop::into_inner(retained))']:
        if token not in d:raise ValueError('lost actual domain owner: '+token)
    for token in ['modpixel_mpss_preauth;','_modem_power:Option<pixel_mpss_preauth::Power>',
                  'pixel_mpss_preauth::start(&d,&mutstate,dict,cmd_db)','pixel_mpss_preauth::fault(self,0x20)',
                  'ifpixel_mpss_preauth::started()','pixel_mpss_preauth::show(&unsafe{Display::new(registers.pixels)},self)',
                  'dictasusize!=0x41000000','cmd_dbasusize!=0x41001000',
                  'self._linux_rmtfs.as_ref().unwrap().phase()!=pixel_linux_rmtfs::Phase::Failed']:
        if token not in e:raise ValueError('lost boot entry binding: '+token)
    if e.index('pixel_mpss_smem::prepare(&d,&mutstate)')>e.index('pixel_mpss_preauth::start(&d,&mutstate,dict,cmd_db)'):raise ValueError('preauth before memory')
    if 'sunfish-native-startup-mpss-preauth=["sunfish-native-startup-mpss-smp2p"]'not in c:raise ValueError('wrong explicit feature dependency')
def audit():
    spec=importlib.util.spec_from_file_location('preauth_inherited_source',HERE/'audit-native-mpss-smem-source.py');base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base);prior=base.audit()
    texts=[(ROOT/p).read_text()for p in PATHS];check(*texts)
    mutations=[(0,'!pixel_smmu_install::preboot_faults_clear()','false'),
               (0,'self.database.check()','Ok(())'),(0,'s._modem_power = Some(power)','s._modem_power = None'),
               (0,'v.apcs.add(3)','v.apcs.add(4)'),(0,'if verify(s).is_err()','if false'),
               (0,'pixel_ipa_receive::poll_pending(s);','/* omit pending delivery */'),
               (0,'if !started()','if false'),(2,'pixel_mpss_preauth::fault(self, 0x20);','/* ignore late fault */')]
    for i,before,after in mutations:
        if before not in texts[i]:raise ValueError('missing source mutation: '+before)
        changed=list(texts);changed[i]=changed[i].replace(before,after)
        try:check(*changed)
        except ValueError:pass
        else:raise ValueError('source mutation accepted: '+before)
    proofs={}
    for folder,file in [('mpss-linux-power','result.json'),('mpss-linux-smp2p','result.json'),('mpss-preauth-entry','result.json')]:
        path=ROOT/'.cache/sunfish-native-wifi/private'/folder/file;j=json.loads(path.read_text())
        if j['result']!='passed' or j['device_actions']:raise ValueError('invalid offline proof '+folder)
        for name,h in j['source_sha256'].items():
            if not source_matches_compatible(name,h):raise ValueError('stale proof '+folder+': '+name)
        proofs[folder]=sha(path)
    return {'scope':'Concrete retained pre-auth power/QMP/SMP2P root entry; no MPSS/PAS4, GLINK service publication or new host DMA transfer.',
            'source_sha256':{p:sha(ROOT/p)for p in PATHS},'proof_sha256':proofs,'source_mutations_rejected':len(mutations),
            'predecessor_source_audit':prior,'cold_contract_required':True,'physical_preauth_proven':False,'mpss_start':False,'production_ready':False}
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args();a.output.write_text(json.dumps(audit(),indent=2)+'\n');print('PASS: concrete pre-auth root entry and eight rejected source mutations. No device action.')
