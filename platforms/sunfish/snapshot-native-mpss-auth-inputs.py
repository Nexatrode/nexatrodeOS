#!/usr/bin/env python3
"""Capture concrete compiler/review inputs before the final Cargo invocation."""
from pathlib import Path
import argparse
import hashlib
import json
import time

ROOT = Path(__file__).resolve().parents[2]


def identity(path):
    return dict(bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest())


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output-dir', type=Path, required=True)
    p.add_argument('--inputs', type=Path, required=True)
    a = p.parse_args()
    out = a.output_dir.resolve()
    dest = out / 'precompile-inputs.json'
    if dest.exists() or (out / 'cargo-build.log').exists() or (out / 'manifest.json').exists():
        raise ValueError('fresh experiment directory required before compilation')
    paths = set((ROOT / 'rust/src').rglob('*.rs'))
    paths.update((ROOT / 'rust/targets').glob('*.json'))
    paths.update((ROOT / 'rust' / name) for name in ['Cargo.toml', 'Cargo.lock'])
    if (ROOT / 'rust/build.rs').is_file(): paths.add(ROOT / 'rust/build.rs')
    paths.update((ROOT / 'tests').glob('*.rs'))
    paths.update(p for p in (ROOT / 'platforms/sunfish').rglob('*')
                 if p.is_file() and p.suffix in ['.py', '.sh', '.system', '.json', '.h', '.inc', '.dts'])
    paths.update(p for p in a.inputs.resolve().iterdir() if p.is_file())
    paths.update(out / name for name in ['private-ipa_fws.mbn', 'private-modem.mbn'])
    paths.update(p for p in (out / 'sdk').rglob('*') if p.is_file())
    paths.update(p for p in (out / 'loader-checkpoint-source').rglob('*') if p.is_file())
    paths.update([out / 'loader-relocated-build/loader.elf', out / 'loader-sdk-profile.json'])
    previous = json.loads((ROOT / '.cache/sunfish-native-wifi/private/native-startup-mpss-smem/manifest.json').read_text())
    paths.update(Path(p) for p in previous['environment_identities'])
    missing = [str(p) for p in paths if not p.is_file()]
    if missing:
        raise ValueError('missing reviewed inputs: ' + ', '.join(missing))
    result = dict(files={str(p.resolve()): identity(p) for p in sorted(paths)},
                  captured_unix_ns=time.time_ns(), before_cargo_log_exists=False)
    dest.write_text(json.dumps(result, indent=2) + '\n')
    print(f'Snapshotted {len(paths)} reviewed inputs before Cargo; no device action.')


if __name__ == '__main__':
    main()
