#!/usr/bin/env python3
"""Exercise actual root lifecycle/policy with CPU receipts; never device I/O."""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/mpss-auth-root'
HARNESS = ROOT / 'tests/pixel_mpss_auth_root_test.rs'
FEATURES = ['sunfish-linux-ipa-qmi', 'sunfish-linux-rmtfs', 'sunfish-linux-mpss-auth',
            'sunfish-linux-mpss-glink', 'sunfish-native-startup-linux-rmtfs',
            'sunfish-native-startup-mpss-smem', 'sunfish-native-startup-mpss-smp2p',
            'sunfish-native-startup-mpss-auth']
FLAGS = ['--edition=2024', '--test', '-D', 'warnings', '-C', 'opt-level=2', '--cfg', 'mpss_auth_root_tests']
for feature in FEATURES:
    FLAGS += ['--cfg', f'feature="{feature}"']


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def deps(path):
    for a, b in re.findall(r'#\[path\s*=\s*"([^"]+)"\]|include!\("([^"]+)"\)', path.read_text()):
        yield (path.parent / (a or b)).resolve()


def absolute(text, path):
    for name in re.findall(r'"([^"]+\.rs)"', text):
        target = (path.parent / name).resolve()
        if target.is_file():
            text = text.replace('"' + name + '"', json.dumps(str(target)))
    return text


def expand(path):
    text = re.sub(r'include!\("([^"]+)"\);',
                  lambda match: expand((path.parent / match[1]).resolve()), path.read_text())
    return absolute(text, path)


def main():
    os.umask(0o077)
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    if OUT.stat().st_mode & 0o077:
        raise ValueError('private output required')
    paths = {HARNESS, Path(__file__).resolve(), ROOT / 'rust/src/pixel_mpss_auth.rs',
             ROOT / 'rust/src/sunfish_native_startup_preflight.rs', ROOT / 'rust/Cargo.toml'}
    pending = [HARNESS]
    while pending:
        for path in deps(pending.pop()):
            if path not in paths:
                paths.add(path)
                pending.append(path)
    hashes = {str(p.relative_to(ROOT)): sha(p) for p in sorted(paths)}
    env = dict(os.environ,
               SUNFISH_LINUX_RMTFS_C=str(OUT.parent / 'linux-rmtfs/linux-scm.txt'),
               SUNFISH_RMTFS_WIRE_C=str(OUT.parent / 'linux-rmtfs-server/wire.txt'),
               SUNFISH_GLK_MEMORY_C=str(OUT.parent / 'mpss-glink-memory-linux'))

    def run(args, name, require_success=True):
        result = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)
        (OUT / (name + '.log')).write_text(result.stdout + result.stderr)
        if require_success and result.returncode:
            raise SystemExit(f'{name} failed: {OUT / (name + ".log")}')
        return result

    rustc = shutil.which('rustc')
    run([rustc, *FLAGS, str(HARNESS), '-o', str(OUT / 'root-tests')], 'root-build')
    tests = run([str(OUT / 'root-tests'), '--test-threads=1'], 'root-tests')
    groups = len(re.findall(r'test .*root_auth_.* \.\.\. ok', tests.stdout))
    if groups != 10:
        raise ValueError('all ten root lifecycle/IRQ/handoff groups required')
    total = int(re.search(r'test result: ok\. (\d+) passed', tests.stdout)[1])
    gate = ROOT / 'rust/src/pixel_linux_rmtfs.rs'
    policy = ROOT / 'rust/src/pixel_mpss_auth_irq.rs'
    boot = ROOT / 'rust/src/sunfish_mpss_linux_boot.rs'
    changes = {
        'retain-cold-after-auth': (gate, '.compare_exchange(1, 3,', '.compare_exchange(1, 1,'),
        'receipt-without-submission': (gate, '.compare_exchange(3, 4,', '.compare_exchange(1, 4,'),
        'stopped-after-auth': (gate, 'modem_stopped: phase == 1,', 'modem_stopped: true,'),
        'ignore-changed-root-maps': (gate, 'mappings() != Ok(self.maps)', 'false'),
        'lose-shared-badge-routing': (policy, '(badge & OWNED, badge & !OWNED)', '(badge & OWNED, badge)'),
        'ignore-watchdog': (policy, 'pending & WDOG == 0', 'true'),
        'poll-without-delivered-irq': (policy, 'pending & SMP != 0', 'true'),
        'ignore-external-quarantine': (boot, 'self.fail(Error::Resource);', '/* omitted terminal transition */'),
    }
    for name, (target, before, after) in changes.items():
        text = target.read_text()
        if before not in text:
            raise ValueError('missing mutation anchor: ' + name)
        folder = OUT / name
        folder.mkdir(exist_ok=True)
        module = folder / 'module.rs'
        module.write_text(absolute(text.replace(before, after), target))
        mutant = expand(HARNESS).replace(json.dumps(str(target)), json.dumps(str(module)))
        test = folder / 'test.rs'
        test.write_text(mutant)
        run([rustc, *FLAGS, '-A', 'unused_variables', str(test), '-o', str(folder / 'test')], name + '-build')
        if run([str(folder / 'test'), '--test-threads=1'], name, False).returncode == 0:
            raise ValueError('behavioral mutation accepted: ' + name)
    for rel, expected in hashes.items():
        path = ROOT / rel
        if sha(path) != expected:
            raise ValueError('source changed during validation: ' + rel)
        dest = OUT / 'source' / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, dest)
    result = dict(
        result='passed', source_sha256=hashes, root_groups=groups,
        total_host_groups=total, mutations_rejected=list(changes), device_actions=[],
        scope='Actual root Gate and lifecycle, closed IRQ policy, actual sealed loader/SMEM receipts in CPU fixtures. Numeric injected root mappings; no fixed VA access, physical SMC/IRQ or native startup proof.',
        no_host_fixed_mapping=True, physical_authentication_proven=False,
        physical_ready_interrupt_proven=False, native_wifi_proven=False)
    (OUT / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(f'PASS: {groups} root groups, {total} total host groups, {len(changes)} behavioral mutations rejected; no device action.')


if __name__ == '__main__':
    main()
