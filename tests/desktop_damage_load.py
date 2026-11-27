#!/usr/bin/env python3
"""Run damage/focus checks and normal-build drag measurements under bounded CPU load.

Uses only an already-running, explicitly disposable desktop fixture. Diagnostic
builds check pixels; normal builds additionally measure 15 seconds of held drag.
No application messages are sent and no personal data is provisioned.
"""
import argparse
import json
import os
from pathlib import Path
import resource
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--guest', type=Path, required=True)
    parser.add_argument('--label', required=True)
    args = parser.parse_args()
    guest = args.guest.resolve()
    assert guest.is_relative_to(ROOT / 'build/desktop-apps')
    assert args.label.replace('-', '').isalnum()
    session = json.loads((guest / 'session.json').read_text())
    assert session['saved_data_used'] is False
    data = Path(session['bundle']) / 'Data'
    assert data.parent.parent == guest
    manifest = json.loads((data / 'build-manifest.json').read_text())
    assert manifest['development_storage_key'] and manifest['desktop_resize']
    commands = [[sys.executable, str(ROOT / 'tests/desktop_damage_input.py'),
                 '--guest', str(guest), '--label', args.label]]
    if not manifest['gpu_verify']:
        commands.append([sys.executable, str(ROOT / 'tests/desktop_gpu_motion.py'),
                         '--guest', str(guest), '--label', args.label,
                         '--seconds', '15', '--hz', '120', '--require-pacing',
                         '--idle-seconds', '3'])
    # Self-terminating children bound load even if this coordinator is stopped.
    code = ("import hashlib,time; end=time.monotonic()+180; data=b'disposable-load'*4096\n"
            "while time.monotonic()<end: hashlib.sha256(data).digest()")
    start = time.monotonic()
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    workers = []
    passed = False
    try:
        for _ in range(6):
            workers.append(subprocess.Popen([sys.executable, '-c', code],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        for command in commands:
            assert all(p.poll() is None for p in workers), 'load expired before test'
            subprocess.run(command, cwd=ROOT, check=True, timeout=150)
        assert all(p.poll() is None for p in workers), 'load expired during test'
        passed = True
    finally:
        for p in workers:
            if p.poll() is None:
                p.terminate()
        for p in workers:
            p.wait()
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        result = dict(label=args.label, passed=passed, saved_data_used=False,
                      gpu_verify=manifest['gpu_verify'], workers=len(workers),
                      bounded_seconds=180, elapsed_seconds=time.monotonic()-start,
                      child_cpu_seconds=after.ru_utime+after.ru_stime-usage.ru_utime-usage.ru_stime,
                      load_average=list(os.getloadavg()),
                      worker_exit_codes=[p.returncode for p in workers])
        (guest / ('damage-load-'+args.label+'.json')).write_text(json.dumps(result, indent=2)+'\n')
        print('LOAD RESULT', json.dumps(result), flush=True)


if __name__ == '__main__':
    main()
