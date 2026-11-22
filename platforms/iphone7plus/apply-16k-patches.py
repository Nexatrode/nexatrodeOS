#!/usr/bin/env python3
"""Apply incremental 16 KiB changes only to their recorded source versions."""
import hashlib
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
manifest = json.loads(Path(__file__).with_name('16k-patches.json').read_text())
# Validate every tree first; never partially overwrite unknown local changes.
pending = []
for name, item in manifest.items():
    states = []
    for rel, hashes in item['files'].items():
        file = root / item['path'] / rel
        digest = hashlib.sha256(file.read_bytes()).hexdigest()
        states.append('after' if digest == hashes['after'] else 'before' if digest == hashes['before'] else 'unknown')
    if all(s == 'after' for s in states):
        print(f'{name}: 16 KiB patch already applied')
    elif all(s == 'before' for s in states):
        pending.append(item)
    else:
        raise SystemExit(f'{name}: source differs from both recorded versions; reconcile local edits before applying')
for item in pending:
    subprocess.run(['git', 'apply', '--check', str(root / item['patch'])], cwd=root / item['path'], check=True)
for item in pending:
    subprocess.run(['git', 'apply', str(root / item['patch'])], cwd=root / item['path'], check=True)
