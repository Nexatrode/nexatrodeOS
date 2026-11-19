#!/usr/bin/env python3
"""Exercise the bounded post-auth session profile offline; never device I/O."""
from pathlib import Path
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/mpss-runtime-services'
HARNESS = ROOT / 'tests/sunfish_ipa_linux_qmi_link_test.rs'
OWNER_HARNESS = ROOT / 'tests/sunfish_linux_rmtfs_server_test.rs'
FEATURES = [
    'sunfish-linux-ipa-qmi',
    'sunfish-linux-rmtfs',
    'sunfish-linux-mpss-auth',
    'sunfish-linux-mpss-glink',
    'sunfish-native-startup-mpss-runtime',
]
FLAGS = ['--edition=2024', '--test', '-D', 'warnings', '-C', 'opt-level=2']
for feature in FEATURES:
    FLAGS += ['--cfg', f'feature="{feature}"']


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args, label, env=None, success=True):
    result = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)
    (OUT / (label + '.log')).write_text(result.stdout + result.stderr)
    if success and result.returncode:
        raise SystemExit(f'{label} failed; see {OUT / (label + ".log")}')
    return result


def main():
    os.umask(0o077)
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    if OUT.stat().st_mode & 0o077:
        raise ValueError('private output required')
    source = ROOT / 'rust/src/qcom_glink_qmi.rs'
    test_source = ROOT / 'tests/sunfish_glink_qmi_test.rs'
    hashes = {str(path.relative_to(ROOT)): sha(path) for path in (
        HARNESS, source, test_source, OWNER_HARNESS,
        ROOT / 'tests/sunfish_linux_rmtfs_server_cases.rs',
        ROOT / 'tests/sunfish_linux_rmtfs_fixture.rs',
        ROOT / 'tests/sunfish_ipa_linux_qmi_owner_test.rs',
    )}
    rmtfs_reference_env = dict(os.environ, RUST_MIN_STACK='16777216')
    run(['python3', str(ROOT / 'platforms/sunfish/test-native-linux-rmtfs-server.py')],
        'rmtfs-reference-and-regressions', rmtfs_reference_env)
    run(['python3', str(ROOT / 'platforms/sunfish/test-native-ipa-linux-qmi.py')], 'qmi-reference')
    link_script = ROOT / 'platforms/sunfish/test-native-ipa-linux-qmi-link.py'
    spec = importlib.util.spec_from_file_location('mpss_runtime_qmi_link', link_script)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    env = dict(os.environ, SUNFISH_LINUX_QMI_LINK_C=str(module.wire_reference()))
    rustc = shutil.which('rustc')
    run([rustc, *FLAGS, str(HARNESS), '-o', str(OUT / 'runtime-tests')], 'host-build', env)
    tests = run([str(OUT / 'runtime-tests'), '--test-threads=1'], 'host-tests', env)
    count = int(re.search(r'test result: ok\. (\d+) passed', tests.stdout)[1])
    required = [
        'post_auth_runtime_session_has_no_early_publication_or_tftp',
        'post_auth_runtime_profile_does_not_change_the_auth_only_session',
    ]
    for name in required:
        if f'test {name} ... ok' not in tests.stdout:
            raise ValueError('required runtime service case missing: ' + name)
    # Exercise the actual existing modem Owner and RAM-backed RMTFS Server
    # under the runtime profile. This verifies RemoteStorage becomes ready only
    # after binding the retained server and that SERVREG is staged once.
    rmtfs_out = OUT / 'owner-rmtfs'
    rmtfs_out.mkdir(parents=True, exist_ok=True, mode=0o700)
    rmtfs_env = dict(os.environ,
        SUNFISH_LINUX_RMTFS_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs/linux-scm.txt'),
        SUNFISH_RMTFS_WIRE_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs-server/wire.txt'))
    owner_flags = [rustc, '--edition=2024', '--test', '-D', 'warnings', '-C', 'opt-level=2']
    for feature in [
        'sunfish-linux-ipa-qmi', 'sunfish-linux-rmtfs',
        'sunfish-linux-mpss-glink', 'sunfish-native-startup-mpss-runtime',
    ]:
        owner_flags += ['--cfg', f'feature="{feature}"']
    owner_env = dict(rmtfs_env, RUST_MIN_STACK='16777216')
    run(owner_flags + [str(OWNER_HARNESS), '-o', str(rmtfs_out / 'tests')],
        'owner-rmtfs-build', owner_env)
    owner_tests = run([str(rmtfs_out / 'tests'), '--test-threads=1'],
                      'owner-rmtfs-tests', owner_env)
    owner_count = int(re.search(r'test result: ok\. (\d+) passed', owner_tests.stdout)[1])
    for name in [
        'post_auth_runtime_keeps_rmtfs_unavailable_until_bound_and_stages_servreg_once',
        'post_auth_runtime_rejects_duplicate_servreg_preparation',
    ]:
        if f'{name} ... ok' not in owner_tests.stdout:
            raise ValueError('required owner/RMTFS integration case missing: ' + name)
    if any(sha(ROOT / rel) != digest for rel, digest in hashes.items()):
        raise ValueError('source changed during validation')
    result = {
        'result': 'passed',
        'tests_passed': count,
        'owner_rmtfs_tests_passed': owner_count,
        'source_sha256': hashes,
        'device_actions': [],
        'scope': 'CPU-only QRTR session profile; no boot, GLINK MMIO, TFTP transfer, firmware or WLAN proof.',
        'tftp_service_published': False,
        'native_wifi_proven': False,
    }
    (OUT / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(f'PASS: {count} GLINK/QMI cases and {owner_count} owner/RMTFS cases including runtime profile, delayed RMTFS readiness, and auth-only preservation; no device action.')


if __name__ == '__main__':
    main()
