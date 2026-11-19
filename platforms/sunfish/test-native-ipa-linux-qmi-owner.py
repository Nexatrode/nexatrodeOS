#!/usr/bin/env python3
"""Linux QMI inside the real serialized FIFO/IRQ/service owner, offline only."""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi-owner'
FEATURE = 'feature="sunfish-linux-ipa-qmi"'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(args, label, env=None, success=True):
    result = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)
    (OUT / (label + '.log')).write_text(result.stdout + result.stderr)
    if success and result.returncode:
        raise SystemExit(f'{label}: exit {result.returncode}; see {OUT / (label + ".log")}')
    return result


def paths(text):
    def target(match):
        return '#[path="' + str((ROOT / 'tests' / match[1]).resolve()) + '"]'
    return re.sub(r'#\[path\s*=\s*"([^"]+)"\]', target, text)


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    run(['python3', str(ROOT / 'platforms/sunfish/test-native-ipa-linux-qmi-link.py')], 'reference-and-link')
    rustc = shutil.which('rustc')
    flags = [rustc, '--edition=2024', '-D', 'warnings', '--cfg', FEATURE]
    test = ROOT / 'tests/sunfish_ipa_linux_qmi_owner_test.rs'
    legacy = ROOT / 'tests/sunfish_modem_owner_test.rs'
    fifo = ROOT / 'tests/sunfish_glink_fifo_test.rs'
    binary = OUT / 'owner-test'
    run(flags + ['--test', str(test), '-o', str(binary)], 'rust-build')
    result = run([str(binary), 'linuxowner_'], 'owner-tests')
    groups = int(re.search(r'test result: ok\. (\d+) passed;', result.stdout)[1])
    all_tests = run([str(binary)], 'feature-regression')
    total = int(re.search(r'test result: ok\. (\d+) passed;', all_tests.stdout)[1])
    run([rustc, '--edition=2024', '-D', 'warnings', '--test', str(legacy), '-o', str(OUT / 'legacy-test')], 'legacy-build')
    old = run([str(OUT / 'legacy-test')], 'legacy-regression')
    regressions = int(re.search(r'test result: ok\. (\d+) passed;', old.stdout)[1])
    if total != groups + regressions:
        raise ValueError('Unexpected feature regression test count')
    declarations = fifo.read_text().split('use qcom_glink_fifo::', 1)[0]
    names = ['qcom_modem_handlers', 'qcom_modem_owner', 'sunfish_ipa_linux_config',
             'sunfish_ipa_linux_memory', 'sunfish_ipa_linux_qmi']
    wrapper = OUT / 'native-owner.rs'
    wrapper.write_text('#![no_std]\n' + paths(declarations) + '\n' + '\n'.join(
        f'#[path="{ROOT}/rust/src/{name}.rs"]pub mod {name};' for name in names) + r'''
pub struct LiveGate(pub fn()->Result<qcom_glink_fifo::Observation,qcom_glink_fifo::Fault>);
impl qcom_glink_native_io::Gate for LiveGate {
 fn observe(&mut self)->Result<qcom_glink_fifo::Observation,qcom_glink_fifo::Fault>{(self.0)()}
}
pub struct Irqs {pub ack:fn(u8)->Result<(),qcom_modem_owner::Error>,pub schedule:fn()->Result<(),qcom_modem_owner::Error>}
impl qcom_modem_owner::Interrupts for Irqs {
 fn acknowledge(&mut self,c:u8)->Result<(),qcom_modem_owner::Error>{(self.ack)(c)}
 fn schedule(&mut self)->Result<(),qcom_modem_owner::Error>{(self.schedule)()}
}
pub type Native=qcom_modem_owner::Owner<qcom_glink_native_io::NativeIo<LiveGate>>;
pub fn attach_native(a:qcom_glink_fifo::Adapter<qcom_glink_native_io::NativeIo<LiveGate>>,g:u64,s:u32,hz:u64)->Result<Native,qcom_modem_owner::Error>{
 Native::attach_linux_ipa(a,g,s,hz)
}
pub fn step_native(owner:&mut Native,mask:u64,now:u64,irqs:&mut Irqs)->Result<qcom_modem_owner::LinuxIpaReport,qcom_modem_owner::Error>{
 owner.notified(mask,irqs)?;owner.service_linux_ipa_step(now,irqs)?;Ok(owner.linux_ipa_report().unwrap())
}
''')
    run(flags + ['--crate-type=lib', '--target=aarch64-unknown-none', str(wrapper),
                 '-o', str(OUT / 'native-owner-aarch64.rlib')], 'aarch64-native-owner')
    owner = ROOT / 'rust/src/qcom_modem_owner.rs'
    child = ROOT / 'rust/src/sunfish_ipa_linux_qmi_owner.rs'
    original = child.read_text()
    flattened = test.read_text().replace('include!("sunfish_modem_owner_test.rs");',
        legacy.read_text().replace('include!("sunfish_glink_fifo_test.rs");', fifo.read_text()))
    flattened = paths(flattened)
    mutants = {
        'late-timeout-boundary': ('now.wrapping_sub(since) >= self.timeout', 'now.wrapping_sub(since) > self.timeout'),
        'skip-protocol-fault': ('let _ = core.fail(self.generation);', 'let _ = core.state();'),
        'drop-held-response': ('runtime.response = Some(handle);', 'let _ = handle;'),
        'replace-held-response': ('if runtime.response.is_some() {', 'if false && runtime.response.is_some() {'),
        'queue-flag-on-backpressure': ('return Ok(LinuxIpaProgress::WaitingQueue)', '{pending.queued=true;return Ok(LinuxIpaProgress::WaitingQueue)}'),
        'omit-continuation': ('self.schedule(interrupts)?;', 'let _ = interrupts;'),
        'complete-after-unwind': ('protocol_complete: !failed', 'protocol_complete: true'),
    }
    for label, (before, after) in mutants.items():
        if original.count(before) != 1:
            raise ValueError('Mutation not unique: ' + label)
        folder = OUT / label
        folder.mkdir(exist_ok=True)
        shutil.copyfile(owner, folder / owner.name)
        (folder / child.name).write_text(original.replace(before, after))
        source = folder / 'test.rs'
        source.write_text(flattened.replace(str(owner), str(folder / owner.name)))
        exe = folder / 'test'
        run(flags + ['--test', str(source), '-o', str(exe)], label + '-build')
        if run([str(exe), 'linuxowner_'], label, success=False).returncode == 0:
            raise ValueError('Mutation accepted: ' + label)
    selected = [str(p.relative_to(ROOT)) for p in [test, legacy, fifo, owner, child]] + [
        'rust/src/sunfish_ipa_linux_qmi_fifo.rs', 'rust/src/qcom_glink_fifo.rs',
        'rust/src/sunfish_ipa_linux_qmi_link.rs', 'rust/src/qcom_glink_qmi.rs',
        'tests/modem_handlers_fixture.rs', 'rust/src/lib.rs', 'rust/Cargo.toml', 'rust/Cargo.lock',
        'platforms/sunfish/test-native-ipa-linux-qmi-owner.py',
        'platforms/sunfish/test-native-ipa-linux-qmi-link.py']
    selected += [str((ROOT / 'tests' / path).resolve().relative_to(ROOT))
                 for path in re.findall(r'#\[path\s*=\s*"([^"]+)"\]', declarations)]
    selected += ['rust/src/' + name + '.rs' for name in names]
    selected = sorted(set(selected))
    for name in selected:
        path = OUT / 'source' / name
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, path)
    upstream = ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi-link'
    proof = OUT / 'link-reference'
    proof.mkdir(exist_ok=True)
    for name in ['manifest.json', 'linux-wire.c', 'linux-wire.txt', 'sel4-library.log']:
        shutil.copyfile(upstream / name, proof / name)
    manifest = {'owner_behavioral_groups': groups, 'regression_groups_each_feature_mode': regressions,
                'source_sha256': {name: sha(ROOT / name) for name in selected},
                'aarch64_native_volatile_owner_monomorphized': True,
                'full_sel4_library_built_by_reference_runner': True,
                'mutations_rejected': list(mutants), 'device_actions': [],
                'scope': 'Real serialized owner, FIFO adapter, IRQ/service dispatch and Linux QMI over guarded host ring memory. AArch64 NativeIo instantiation compiled. Native startup-resource factory, timer wake, MPSS boot and Wi-Fi remain unproven.'}
    manifest['artifacts'] = {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p)} for p in OUT.iterdir() if p.is_file() and p.name != 'manifest.json'}
    manifest['mutation_artifacts'] = {str(p.relative_to(OUT)): {'bytes': p.stat().st_size, 'sha256': sha(p)} for label in mutants for p in (OUT / label).iterdir() if p.is_file()}
    manifest['link_reference'] = {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p)} for p in proof.iterdir() if p.is_file()}
    (OUT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'PASS: {groups} FIFO-owner groups, {regressions} legacy groups in both feature modes, seven rejected mutations, AArch64 native instantiation and seL4 library; no device action.')


if __name__ == '__main__':
    main()
