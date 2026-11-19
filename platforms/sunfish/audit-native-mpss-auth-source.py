#!/usr/bin/env python3
"""Bind the one-shot native auth entry to real retained owners and pinned Linux."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import re
import sys
import tarfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PRIVATE = ROOT / '.cache/sunfish-native-wifi/private'
sys.path.insert(0, str(HERE))
from proof_source_compat import source_matches_compatible
PATHS = ['rust/src/pixel_mpss_auth.rs', 'rust/src/pixel_mpss_auth_irq.rs',
         'rust/src/pixel_linux_rmtfs.rs', 'rust/src/pixel_mpss_preauth.rs',
         'rust/src/pixel_ipa_receive.rs', 'rust/src/pixel_smmu_install.rs',
         'rust/src/sunfish_native_startup_preflight.rs', 'rust/Cargo.toml']


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def compact(text):
    return ''.join(re.sub(r'//[^\n]*', '', re.sub(r'/\*.*?\*/', '', text, flags=re.S)).split())


def check(*texts):
    auth, irq, factory, power, receive, domain, entry, cargo = map(compact, texts)
    required = [
        'include_bytes!(env!("SUNFISH_MPSS_FIRMWARE_PATH"))',
        'metadataasusize!=0x77501000', 'targetasusize!=0x88000000',
        '*var!(mpss_target_paddr:usize=0)asu64!=pas::BASE',
        'maps.metadata_pa+4096,maps.metadata_pa+8192',
        'Window::new(', 'pas::Image::private_reference(MODEM)',
        'STARTED.compare_exchange(0,1,Ordering::AcqRel,Ordering::Acquire)',
        's._linux_rmtfs.as_mut().unwrap().take_boot_parts()',
        '_session:Some(ManuallyDrop::new(session))', '_tftp:tftp.map(ManuallyDrop::new)',
        'boot::Boot::new(GENERATION,loader,r.unstarted.take().unwrap(),)',
        'pixel_linux_rmtfs::abort()', 'b.quarantine()',
        'seL4_ARM_SMC_Call(7,&args,&mutr)', 'ife!=0{returnErr(Fault::Uncertain);}',
        'smccc::invoke_checked(&mutMonitor,&mutDelay,&mutRetry{env:&self.env,guard:g,},*r.words(),)',
        'self.env.check(self.guard).map_err(|_|Fault::Uncertain)',
        'iflife!=smp::Lifecycle::AuthSubmitted', 'elseiflife!=smp::Lifecycle::PreAuth',
        'Some(smp::Lifecycle::AuthSubmitted)ifg.auth_submitted=>Ok(())',
        'Some(smp::Lifecycle::PreAuth)ifPENDING.load(Ordering::Acquire)&SMP==0=>Ok(())',
        'self.resources.acquire()', 'self.resources.release(l)',
        'power::Resources::new(s._modem_power.as_mut().unwrap(),&s._scm_hold,s._firmware_rsc.as_mut().unwrap(),65536,)',
        '!self.domain.retained_quiet()', 'self.rx.error().is_some()',
        'self.rx.startup_report().phase!=sunfish_ipa_linux_receive_start::Phase::Armed',
        '!pixel_smmu_install::preboot_faults_clear()', '!capture_during_call()',
        'g.auth_submitted&&!self.cold_peer_verified',
        'b.loader_report().phase==pas::Phase::AuthAndReset',
        'ifp!=smp::Poll::Waiting{returnErr(0x39);}',
        '.cold_peer_verified=true;',
        'DEFERRED.fetch_or(rest,Ordering::AcqRel)', 'DEFERRED.swap(0,Ordering::AcqRel)',
        'irq_policy::split(b)', 'irq_policy::monitor_healthy(rest,PENDING.load(Ordering::Acquire))',
        'ifpending&WDOG!=0{returnErr(0x31);}',
        'irq_policy::ack_sample(pending,p)', 'irq_policy::ready_sample(&report)',
        'Channel::new(SMP_CHANNEL).irq_ack()', 'Channel::new(WDOG_CHANNEL).irq_ack()',
        'PENDING.fetch_and(!SMP,Ordering::AcqRel)',
        'r.boot.as_mut().unwrap().take_auth_returned()',
        'pixel_linux_rmtfs::auth_returned(r.auth.as_ref().unwrap())',
        'ifcounter().wrapping_sub(r.since)>=r.hz*5{',
        'ifr.ready_irq{', 's._domain.as_mut().unwrap().take_modem_ready()',
        'pixel_linux_rmtfs::running(r.auth.as_ref().unwrap(),r.ready.as_ref().unwrap(),)',
        '"READYRECEIPT"', '"NOGLINKORWLANSTART"']
    for token in required:
        if token not in auth:
            raise ValueError('native auth binding: ' + token)
    if auth.count('seL4_ARM_SMC_Call(') != 1 or auth.count('pixel_mpss_preauth::verify_retained(s)') != 2:
        raise ValueError('sole monitor path and both retained power checks required')
    for forbidden in ['write_volatile', 'transmute', 'copy_nonoverlapping', '.start_control(', '.claim_glink(', '.reset(']:
        if forbidden in auth:
            raise ValueError('unexpected root auth operation: ' + forbidden)
    for token in ['constSMP_CHANNEL:usize=5;', 'constWDOG_CHANNEL:usize=6;',
                  '(badge&OWNED,badge&!OWNED)', 'rest&!0xf==0&&rest&3==0&&pending&WDOG==0',
                  'pending&SMP!=0&&poll==smp::Poll::Observed',
                  'report.ready_seen&&report.peer.and_then(|p|p.slave_kernel).is_some_and(|v|v&2!=0)']:
        if token not in irq:
            raise ValueError('IRQ ownership/evidence binding: ' + token)
    for token in ['r.words()!=&[0x42000205,1,4,0,0,0,0,0]',
                  '.compare_exchange(1,3,Ordering::AcqRel,Ordering::Acquire)',
                  '.compare_exchange(3,4,Ordering::AcqRel,Ordering::Acquire)',
                  '.compare_exchange(4,5,Ordering::AcqRel,Ordering::Acquire)',
                  's.transport_is_pristine()', 'self.phase=Phase::Delegated;']:
        if token not in factory:
            raise ValueError('actual factory lifecycle binding: ' + token)
    for token, text in [
        ('pixel_linux_rmtfs::lifecycle(g).ok_or(smp::Error::Guard)?', power),
        ('pixel_mpss_auth::receive_badge()', receive),
        ('self.adapter.as_mut().ok_or(Error::State)?.modem_smp2p_poll()', domain),
        ('pixel_mpss_preauth::start(&d,&mutstate,dict,cmd_db)', entry),
        ('pixel_mpss_auth::start(&d,&mutstate)', entry),
        ('ifpixel_mpss_auth::started(){pixel_mpss_auth::notified(self,channels);', entry),
        ('#[cfg(feature="sunfish-native-startup-mpss-runtime")]ifself._mpss_runtime.is_some(){pixel_mpss_runtime::notified(self,channels);returnOk(());}', entry),
        ('sunfish-native-startup-mpss-auth=["sunfish-native-startup-mpss-preauth","sunfish-linux-mpss-auth"]', cargo)]:
        if token not in text:
            raise ValueError('retained concrete entry binding: ' + token)
    if entry.index('pixel_mpss_auth::start(&d,&mutstate)') < entry.index('pixel_mpss_preauth::start(&d,&mutstate,dict,cmd_db)'):
        raise ValueError('authentication precedes proven pre-auth composition')


def audit():
    texts = [(ROOT / p).read_text() for p in PATHS]
    check(*texts)
    mutations = [
        (0, '!capture_during_call()', 'false'),
        (0, 'if r.ready_irq {', 'if true {'),
        (0, 'r.hz * 5', 'r.hz * 5000'),
        (0, 'b.quarantine();', '/* ignore external fault */'),
        (0, 'seL4_ARM_SMC_Call(7,', 'seL4_ARM_SMC_Call(8,'),
        (0, 'DEFERRED.fetch_or(rest, Ordering::AcqRel);', '/* lose other IRQs */'),
        (0, 'pixel_mpss_preauth::verify_retained(s)', 'Ok::<(), ()>(())'),
        (2, '.compare_exchange(1, 3,', '.compare_exchange(1, 1,'),
        (6, 'pixel_mpss_auth::notified(self, channels);', '/* drop IRQ owner */'),
    ]
    for index, before, after in mutations:
        if before not in texts[index]:
            raise ValueError('missing source mutation anchor: ' + before)
        changed = list(texts)
        changed[index] = changed[index].replace(before, after)
        try:
            check(*changed)
        except ValueError:
            pass
        else:
            raise ValueError('accepted source mutation: ' + before)
    s = importlib.util.spec_from_file_location('auth_inherited', HERE / 'audit-native-mpss-preauth-source.py')
    preauth = importlib.util.module_from_spec(s)
    s.loader.exec_module(preauth)
    inherited = preauth.audit()
    proofs = {}
    for folder, file in [('mpss-auth-root', 'result.json'), ('mpss-linux-boot', 'result.json'),
                         ('mpss-linux-loader', 'result.json')]:
        path = PRIVATE / folder / file
        data = json.loads(path.read_text())
        if data.get('result') != 'passed' or data.get('device_actions'):
            raise ValueError('invalid offline proof: ' + folder)
        for rel, expected in data['source_sha256'].items():
            if not source_matches_compatible(rel, expected):
                raise ValueError('stale offline proof: ' + folder + '/' + rel)
        proofs[folder] = sha(path)
    archive = ROOT / '.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive) != 'd55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2':
        raise ValueError('wrong selected Linux archive')
    with tarfile.open(archive) as tar:
        names = {m.name.split('/', 1)[1]: m for m in tar if '/' in m.name}
        rel = 'drivers/firmware/qcom/qcom_scm-smc.c'
        smc = tar.extractfile(names[rel]).read()
        source = smc.decode()
        for token in ['#define QCOM_SCM_EBUSY_WAIT_MS 30', '#define QCOM_SCM_EBUSY_MAX_RETRY 20',
                      'retry_count++ > QCOM_SCM_EBUSY_MAX_RETRY', 'QCOM_SCM_INTERRUPTED', 'quirk.state.a6']:
            if token not in source:
                raise ValueError('selected Linux SMCCC mechanics changed: ' + token)
    return dict(scope='Concrete one-shot root PAS4/ready entry. Actual MPSS execution and IRQ readiness remain device tests; no GLINK/WLAN startup.',
                source_sha256={p: sha(ROOT / p) for p in PATHS}, proof_sha256=proofs,
                inherited_source_audit=inherited, source_mutations_rejected=len(mutations),
                linux_scm_smc_sha256=hashlib.sha256(smc).hexdigest(),
                bounded_retry_differences=['Interrupted resumes capped at 32; total monitor calls capped at 64.',
                                          'Unsupported WAITQ statuses fail and retain owners; no guessed WAITQ resume.'],
                device_actions=[], physical_authentication_proven=False, native_wifi_proven=False)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    a.output.write_text(json.dumps(audit(), indent=2) + '\n')
    print('PASS: one-shot native MPSS auth source bindings and nine rejected source mutations; no device action.')
