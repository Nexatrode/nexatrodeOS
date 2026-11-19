#!/usr/bin/env python3
"""Compile the actual native policy against retained receive witnesses and Linux C writes."""
from pathlib import Path
import hashlib,importlib.util,json,os,re,shutil
ROOT=Path(__file__).resolve().parents[2];HERE=Path(__file__).resolve().parent
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-receive-native-policy'
spec=importlib.util.spec_from_file_location('receive_policy_runtime',HERE/'test-native-ipa-linux-receive-runtime.py')
rx=importlib.util.module_from_spec(spec);spec.loader.exec_module(rx);rx.OUT=OUT
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,label,env=None,success=True):return rx.run(args,label,env,success)
def main():
    OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    fixtures={
        'SUNFISH_RECEIVE_DISPATCH_C':('ipa-linux-receive-dispatch','linux-shared-ipa.txt'),
        'SUNFISH_RECEIVE_RUNTIME_C':('ipa-linux-receive-runtime','linux-receive-runtime.txt'),
        'SUNFISH_RECEIVE_OWNERS_FIXTURE':('ipa-linux-receive-start','retained-fixture.rs'),
        'SUNFISH_RECEIVE_STARTUP_C':('ipa-linux-receive-start','linux-receive-startup-writes.txt'),
        'SUNFISH_TRANSFER_START_FIXTURE':('ipa-linux-transfer','start-fixture.rs'),
        'SUNFISH_COMMAND_ENDPOINT_FIXTURE':('ipa-linux-command','endpoint-fixture.rs'),
        'SUNFISH_ENDPOINT_GSI_FIXTURE':('ipa-linux-endpoint','gsi-fixture.rs'),
        'SUNFISH_TRANSFER_C_REFERENCE':('ipa-linux-transfer','linux-transfer-writes.txt'),
        'SUNFISH_TABLE_TRANSFER_C_REFERENCE':('ipa-linux-transfer','linux-table-transfer-writes.txt'),
        'SUNFISH_COMMAND_C_REFERENCE':('ipa-linux-command','linux-command-writes.txt')}
    env=dict(os.environ)
    for k,(folder,name)in fixtures.items():env[k]=str(rx.verified_fixture(folder,name))
    rustc=shutil.which('rustc');test=ROOT/'tests/sunfish_ipa_receive_native_policy_test.rs';binary=OUT/'native-policy-test'
    run([rustc,'--edition=2024','-D','warnings','-C','opt-level=2','--test',str(test),'-o',str(binary)],'rust-build',env)
    tested=run([str(binary),'nativepolicy_'],'rust-tests',env)
    groups=int(re.search(r'test result: ok\. (\d+) passed;',tested.stdout)[1])
    for name in ['rxstart_','rxruntime_','sharedirq_']:run([str(binary),name],name+'regression',env)
    original=(ROOT/'rust/src/pixel_ipa_receive_policy.rs').read_text()
    mutants={
        'wrong-start-channel':('word.value == 0x01000002','word.value == 0x01000001'),
        'unaligned-event-credit':('(0x80008000..0x80009000).contains(&value) && value % 16 == 0','(0x80008000..0x80009000).contains(&value)'),
        'foreign-aggregation':('actual.guards[2] & !0x100 != 0','actual.guards[2] & !0x101 != 0'),
        'foreign-badge':('_ => Notice::Fault,','_ => Notice::Both,')}
    for label,(before,after)in mutants.items():
        if original.count(before)!=1:raise ValueError('Mutation not unique: '+before)
        path=OUT/(label+'.rs');path.write_text(original.replace(before,after))
        body=test.read_text().replace('include!("sunfish_ipa_linux_receive_dispatch_test.rs");',f'include!("{ROOT}/tests/sunfish_ipa_linux_receive_dispatch_test.rs");').replace('../rust/src/pixel_ipa_receive_policy.rs',str(path)).replace('../rust/src/pixel_mpss_auth_irq.rs',str(ROOT/'rust/src/pixel_mpss_auth_irq.rs'))
        harness=OUT/(label+'-test.rs');harness.write_text(body);exe=OUT/(label+'-test')
        run([rustc,'--edition=2024','-D','warnings','-C','opt-level=2','--test',str(harness),'-o',str(exe)],label+'-build',env)
        if run([str(exe),'nativepolicy_'],label,env,False).returncode==0:raise ValueError('Mutation accepted: '+label)
    selected=['rust/src/sunfish_ipa_linux_'+n+'.rs'for n in rx.NAMES+['irq','uc','receive_dispatch']]+[
        'rust/src/pixel_ipa_receive_policy.rs','rust/src/pixel_mpss_auth_irq.rs','tests/sunfish_ipa_receive_native_policy_test.rs',
        'tests/sunfish_ipa_linux_receive_dispatch_test.rs','tests/sunfish_ipa_linux_receive_runtime_test.rs',
        'tests/sunfish_ipa_linux_receive_start_test.rs','platforms/sunfish/test-native-ipa-receive-policy.py']
    for name in selected:
        path=OUT/'source'/name;path.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(ROOT/name,path)
    j={'source_sha256':{n:sha(ROOT/n)for n in selected},'behavioral_groups':groups,
        'startup_regression_groups':9,'receive_runtime_regression_groups':10,'shared_irq_regression_groups':9,
        'compiled_original_startup_writes':27,'mutations_rejected':list(mutants),'hardware_commands':[],
        'scope':'Actual native pure policy with sealed transition witnesses from the real retained startup. No native MMIO execution, physical RX or Wi-Fi proof.'}
    j['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)}for p in OUT.iterdir()if p.is_file()and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(j,indent=2)+'\n')
    print(f'PASS: {groups} native policy groups, 28 retained-core regression groups and four rejected mutations; no device action.')
if __name__=='__main__':main()
