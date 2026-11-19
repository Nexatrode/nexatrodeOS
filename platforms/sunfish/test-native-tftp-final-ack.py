#!/usr/bin/env python3
"""CPU-only first-final-ACK trace checks, feature off and on."""
from pathlib import Path
import hashlib
import importlib.util
import json
import os
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/tftp-final-ack-check-v1'
FEATURE = 'sunfish-tftp-final-ack-observer'

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    spec = importlib.util.spec_from_file_location('transport', ROOT / 'platforms/sunfish/test-native-mpss-linux-transport.py')
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    harness = ROOT / 'tests/sunfish_glink_qmi_test.rs'
    paths = {harness, Path(__file__).resolve(), Path(helper.__file__), ROOT / 'rust/Cargo.toml', ROOT / 'rust/Cargo.lock'}
    pending = [harness, ROOT / 'tests/sunfish_mpss_linux_transport_test.rs']
    paths.update(pending)
    while pending:
        for path in helper.deps(pending.pop()):
            if path not in paths:
                paths.add(path); pending.append(path)
    paths.update(ROOT / p for p in ['rust/src/pixel_mpss_runtime.rs', 'platforms/sunfish/test-native-mpss-runtime-source.py'])
    hashes = {str(p.relative_to(ROOT)): sha(p) for p in sorted(paths)}
    env = dict(os.environ, RUSTUP_TOOLCHAIN='nightly-2026-09-25', RUST_MIN_STACK='16777216')
    results = {}
    for mode in ['off', 'on']:
        features = ['sunfish-native-tftp-server-check', 'sunfish-tftp-endpoint-observer', 'sunfish-tftp-per-peer-reply-experiment', 'sunfish-qrtr-rmtfs-flow-experiment']
        if mode == 'on': features += [FEATURE]
        flags = [arg for f in features for arg in ['--cfg', f'feature="{f}"']]
        cmd = ['rustc', '--edition=2024', '--test', '-D', 'warnings', '-A', 'dead_code', '-A', 'deprecated', '-C', 'opt-level=2', *flags, str(harness), '-o', str(OUT / ('session-' + mode))]
        for step, command in [('build', cmd), ('tests', [str(OUT / ('session-' + mode)), '--test-threads=1'])]:
            p = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True)
            (OUT / f'session-{mode}-{step}.log').write_text(p.stdout + p.stderr)
            if p.returncode: raise RuntimeError(f'{mode} {step}: {(p.stdout + p.stderr)[-5000:]}')
        match = re.search(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', p.stdout)
        assert match
        results[mode] = dict(passed=int(match[1]), ignored=int(match[2]))
        print(mode, results[mode], flush=True)
    for name, digest in hashes.items(): assert sha(ROOT / name) == digest, name
    (OUT / 'result.json').write_text(json.dumps(dict(result='passed', feature=FEATURE, tests=results, source_sha256=hashes, device_actions=[]), indent=2) + '\n')

if __name__ == '__main__': main()
