#!/usr/bin/env python3
"""Offline paired comparison of serialized and independent IPA QMI progress.

Uses actual owner/FIFO/core code and source-extracted Linux/Android callbacks.
Does not build a bootable candidate or access a phone.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
FEATURE = 'sunfish-ipa-independent-server-experiment'
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--output', type=Path, default=ROOT / '.cache/sunfish-native-wifi/private/ipa-independent-server-offline-v1')
a = p.parse_args()
OUT = a.output.resolve()
OUT.mkdir(mode=0o700, parents=True, exist_ok=True)
BASE = ROOT / '.cache/sunfish-native-wifi/private'
ENV = dict(os.environ, RUSTUP_TOOLCHAIN='nightly-2026-09-25', RUST_MIN_STACK='16777216',
           SUNFISH_LINUX_QMI_LINK_C=str(BASE / 'ipa-linux-qmi-link/linux-wire.txt'),
           SUNFISH_LINUX_QMI_C=str(BASE / 'ipa-linux-qmi/linux-qmi.txt'),
           SUNFISH_LINUX_RMTFS_C=str(BASE / 'linux-rmtfs/linux-scm.txt'),
           SUNFISH_RMTFS_WIRE_C=str(BASE / 'linux-rmtfs-server/wire.txt'))

def run(cmd, label):
    (OUT / (label + '-command.json')).write_text(json.dumps(cmd, indent=2) + '\n')
    r = subprocess.run(cmd, cwd=ROOT, env=ENV, capture_output=True, text=True)
    (OUT / (label + '.log')).write_text(r.stdout + r.stderr)
    if r.returncode:
        print((r.stdout + r.stderr)[-8000:])
        raise SystemExit(label + ' failed')
    print(label + ': passed', flush=True)
    return r.stdout

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def closure(selected):
    graph = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())['features']
    selected = set(selected)
    pending = list(selected)
    while pending:
        for name in graph.get(pending.pop(), []):
            if name in graph and name not in selected:
                selected.add(name)
                pending.append(name)
    return sorted(selected)

sources = list((ROOT / 'rust/src').glob('*.rs')) + list((ROOT / 'tests').glob('*.rs')) + [ROOT / 'rust/Cargo.toml', Path(__file__).resolve(), ROOT / 'platforms/sunfish/compare-ipa-registration-reference.py']
hashes = {str(p.relative_to(ROOT)): sha(p) for p in sources}
fixtures = {k: dict(path=v, sha256=sha(Path(v))) for k,v in ENV.items() if k.startswith('SUNFISH_')}
results = {}
for mode in ['baseline', 'candidate']:
    features = closure(['sunfish-qrtr-ipa-flow-experiment', 'sunfish-ipa-independent-response-experiment']
                       + ([FEATURE] if mode == 'candidate' else []))
    flags = ['rustc', '--edition=2024', '--test', '-D', 'warnings', '-A', 'dead_code',
             '-A', 'deprecated', '-C', 'opt-level=2']
    for f in features:
        flags += ['--cfg', f'feature="{f}"']
    for kind, name in [('core', 'sunfish_ipa_linux_qmi_test.rs'),
                       ('owner', 'sunfish_ipa_independent_server_test.rs'),
                       ('link', 'sunfish_ipa_independent_link_test.rs')]:
        binary = OUT / (mode + '-' + kind)
        run(flags + [str(ROOT / 'tests' / name), '-o', str(binary)], mode + '-' + kind + '-build')
        text = run([str(binary), '--test-threads=1', '--nocapture'], mode + '-' + kind + '-tests')
        match = re.search(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', text)
        assert match
        results[mode + '-' + kind] = dict(passed=int(match[1]), ignored=int(match[2]), features=features)
        if kind == 'owner':
            assert f'independent={str(mode == "candidate").lower()} registration published before INIT receipt={str(mode == "candidate").lower()}' in text
    # Compile the same owner with the real volatile backend for AArch64,
    # without std or test-only APIs. This is not the Pixel runtime binary.
    declarations = (ROOT / 'tests/sunfish_glink_fifo_test.rs').read_text().split('use qcom_glink_fifo::', 1)[0]
    declarations = re.sub(r'#\[path\s*=\s*"([^\"]+)"\]',
        lambda m: '#[path="' + str((ROOT / 'tests' / m[1]).resolve()) + '"]', declarations)
    names = ['qcom_modem_handlers', 'qcom_modem_owner', 'sunfish_ipa_linux_config',
             'sunfish_ipa_linux_memory', 'sunfish_ipa_linux_qmi', 'qcom_modem_ram_storage',
             'qcom_modem_storage', 'sunfish_linux_rmtfs', 'sunfish_linux_rmtfs_native',
             'sunfish_linux_rmtfs_server', 'sunfish_lpd']
    wrapper = OUT / (mode + '-native-owner.rs')
    wrapper.write_text('#![no_std]\n' + declarations + '\n' + '\n'.join(
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
    run([f for f in flags if f != '--test'] + ['--crate-type=lib', '--target=aarch64-unknown-none',
        str(wrapper), '-o', str(OUT / (mode + '-native-owner.rlib'))], mode + '-aarch64-owner')
    results[mode + '-aarch64-owner'] = dict(no_std=True, native_io_instantiated=True)
run(['python3', str(ROOT / 'platforms/sunfish/compare-ipa-registration-reference.py'),
     '--output', str(OUT / 'reference')], 'linux-android-registration-reference')
reference = json.loads((OUT / 'reference/registration-reference.json').read_text())
# Source-only guard is intentional: native runtime composition is not part of
# this comparison, even though the no_std implementation can be cross-compiled.
guard = 'compile_error!("independent IPA server experiment is offline only; native composition not validated");'
assert guard in (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
for name, digest in hashes.items():
    assert sha(ROOT / name) == digest, name
(OUT / 'result.json').write_text(json.dumps(dict(result='passed', profiles=results,
    source_sha256=hashes, fixture_sha256=fixtures, registration_reference=reference, device_actions=[], native_boot_allowed=False,
    scope='Real owner/core/session/FIFO comparison; same receipt requirements, independent bounded sockets. Not a firmware or native runtime result.'), indent=2) + '\n')
print('PASS: paired independent client/server comparison; no device action.')
