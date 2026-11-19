#!/usr/bin/env python3
"""Compare paired GLINK barrier/status checks with the feature on and off.

CPU/RAM tests only. The transport suite exercises the real retained native
adapter; the ordinary private-image loader test is deliberately ignored here.
"""
from pathlib import Path
import hashlib
import importlib.util
import json
import os
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/glink-barrier-pair-check-v1'
FEATURE = 'sunfish-glink-barrier-observe-pair'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    os.umask(0o077)
    OUT.mkdir(mode=0o700, parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location(
        'transport_runner', ROOT / 'platforms/sunfish/test-native-mpss-linux-transport.py')
    transport = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(transport)
    fifo = ROOT / 'tests/sunfish_glink_fifo_test.rs'
    paths = {Path(__file__).resolve(), Path(transport.__file__), ROOT / 'rust/Cargo.toml',
             ROOT / 'rust/Cargo.lock', fifo, transport.HARNESS}
    pending = [fifo, transport.HARNESS]
    while pending:
        for path in transport.deps(pending.pop()):
            if path not in paths:
                paths.add(path)
                pending.append(path)
    hashes = {str(p.relative_to(ROOT)): sha(p) for p in sorted(paths)}
    env = dict(os.environ, RUSTUP_TOOLCHAIN='nightly-2026-09-25',
               SUNFISH_LINUX_RMTFS_C=str(OUT.parent / 'linux-rmtfs/linux-scm.txt'),
               SUNFISH_RMTFS_WIRE_C=str(OUT.parent / 'linux-rmtfs-server/wire.txt'),
               SUNFISH_GLK_MEMORY_C=str(OUT.parent / 'mpss-glink-memory-linux'),
               SUNFISH_GLK_TRANSPORT_C=str(OUT.parent / 'mpss-glink-transport-linux'))
    cases = [
        ('fifo', fifo, ['--edition=2024', '-D', 'warnings', '-A', 'deprecated', '--test']),
        ('transport', transport.HARNESS, transport.FLAGS + ['-A', 'deprecated', '-A', 'dead_code']),
    ]
    results = {}
    for name, harness, flags in cases:
        for mode in ['off', 'on']:
            label = f'{name}-{mode}'
            selected = flags + (['--cfg', f'feature="{FEATURE}"', '--cfg', 'feature="sunfish-glink-combined-cursor-sample"'] if mode == 'on' else ['--cfg', 'feature="sunfish-glink-combined-cursor-sample"'])
            commands = [
                ('build', ['rustc', *selected, str(harness), '-o', str(OUT / label)]),
                ('tests', [str(OUT / label), '--test-threads=1']),
            ]
            for step, command in commands:
                p = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True)
                (OUT / f'{label}-{step}.log').write_text(p.stdout + p.stderr)
                if p.returncode:
                    raise RuntimeError(f'{label}-{step} failed; see {OUT}')
            match = re.search(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', p.stdout)
            assert match, label
            results[label] = {'passed': int(match[1]), 'ignored': int(match[2])}
            print(label, results[label], flush=True)
    for rel, digest in hashes.items():
        assert sha(ROOT / rel) == digest, f'changed during test: {rel}'
    result = {'result': 'passed', 'feature': FEATURE, 'tests': results,
              'source_sha256': hashes, 'device_actions': [],
              'warning_exceptions': 'Existing deprecated atomic API; existing dead code in transport harness.',
              'scope': 'FIFO faults and cursor checks, actual SMEM/controller checks, native retained domain with corruption in each of eight pages and injected corruption inside the paired operation. CPU RAM only.',
              'limitations': 'No physical timing, peer behavior or WLAN proof. Private-image loader comparison is a separate prior test.'}
    (OUT / 'result.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
