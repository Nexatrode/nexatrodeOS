#!/usr/bin/env python3
"""Reproduce the isolated RAM-only NC SDK; never access the phone.

Requires the existing Sunfish checkouts, LLVM, venv and pinned Rust toolchain.
The complete patches include local path dependencies and their lockfile.
The verified relocated loader is overlaid by the final image profile, separately.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path(__file__).with_name('table-normal-nc-sources.json')
PROFILE = ROOT / '.cache/sunfish-native-wifi/private/native-wrq-table-normal-nc-v1'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree_sha(root):
    """Pin whole source trees, including local board changes outside patch hunks."""
    digest = hashlib.sha256()
    files = []
    for folder, dirs, names in os.walk(root):
        at_root = Path(folder) == root
        dirs[:] = sorted(d for d in dirs if d not in {
            '.git', '__pycache__', '.pytest_cache'
        } and not (at_root and (d in {'target', 'release', 'build'} or d.startswith('build-'))))
        for name in names:
            if name == '.DS_Store' or name.endswith(('.orig', '.rej', '.pyc')):
                continue
            path = Path(folder) / name
            files.append(path)
    for path in sorted(files):
        rel = path.relative_to(root).as_posix()
        # Include path boundaries as well as file content; no concatenation ambiguity.
        digest.update(rel.encode() + b'\0' + bytes.fromhex(sha(path)))
    return digest.hexdigest()


def run(args, **kwargs):
    subprocess.run([str(x) for x in args], check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare-only', action='store_true')
    args = parser.parse_args()
    manifest = json.loads(MANIFEST.read_text())
    for rel, expected in manifest['external_inputs'].items():
        if sha(ROOT / rel) != expected:
            raise ValueError(f'external build input changed: {rel}')
    rust = Path.home() / '.cargo/git/checkouts/rust-sel4-d7edb8214cac2bde/dbe6445'
    sources = {'sel4': ROOT / '.cache/sel4-src-16.0-sunfish',
               'rust-sel4': rust, 'microkit': ROOT / '.cache/microkit-src-2.3.1-sunfish'}
    targets = {'sel4': PROFILE / 'kernel-source', 'rust-sel4': PROFILE / 'rust-source',
               'microkit': PROFILE / 'microkit-source'}
    for name, entry in manifest['sources'].items():
        base, dest = sources[name], targets[name]
        head = subprocess.check_output(['git', '-C', str(base), 'rev-parse', 'HEAD'], text=True).strip()
        if head != entry['base_head']:
            raise ValueError(f'{name}: unexpected base revision {head}')
        patch = ROOT / entry['patch']
        if sha(patch) != entry['patch_sha256']:
            raise ValueError(f'{name}: patch hash changed')
        if tree_sha(base) != entry['base_tree_sha256']:
            raise ValueError(f'{name}: unpinned base source tree change')
        for rel, hashes in entry['files'].items():
            if sha(base / rel) != hashes['base_sha256']:
                raise ValueError(f'{name}: base file changed: {rel}')
        if not dest.exists():
            dest.mkdir(parents=True)
            run(['rsync', '-a', '--exclude=.git', '--exclude=/build/', '--exclude=/build-*/',
                 '--exclude=/target/', '--exclude=/release/', '--exclude=__pycache__',
                 str(base) + '/', str(dest) + '/'])
            with patch.open('rb') as stream:
                run(['patch', '--batch', '-p1', '-d', dest], stdin=stream)
        # A partial preparation is rejected, never assumed to be a valid SDK.
        for rel, hashes in entry['files'].items():
            if sha(dest / rel) != hashes['patched_sha256']:
                raise ValueError(f'{name}: patched file changed or incomplete: {rel}')
        if tree_sha(dest) != entry['patched_tree_sha256']:
            raise ValueError(f'{name}: unpinned patched source tree change')
    print('Verified pinned Sunfish base revisions, patch hashes and patched source files.', flush=True)
    if args.prepare_only:
        return
    env = dict(os.environ, RUSTUP_TOOLCHAIN='nightly-2026-09-25', CARGO_NET_OFFLINE='true',
               SUNFISH_SOURCE_ROOT=str(ROOT))
    env['PATH'] = f'{ROOT}/.cache/sel4-venv/bin:/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:' + env['PATH']
    cwd = targets['microkit']
    run(['cargo', 'test', '-p', 'microkit-tool', '--locked', '--offline'], cwd=cwd, env=env)
    run(['cargo', 'build', '-p', 'microkit-tool', '--release', '--locked', '--offline'], cwd=cwd, env=env)
    run([ROOT / '.cache/sel4-venv/bin/python', 'build_sdk.py', '--sel4', targets['sel4'],
         '--llvm', '--boards', 'sunfish', '--configs', 'debug', '--version',
         '2.3.1-sunfish-table-normal-nc', '--skip-tool', '--skip-docs', '--skip-tar'], cwd=cwd, env=env)
    print('Isolated SDK built. Final image packaging and relocated-loader audit remain separate.')


if __name__ == '__main__':
    main()
