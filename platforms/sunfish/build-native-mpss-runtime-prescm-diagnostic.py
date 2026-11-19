#!/usr/bin/env python3
"""Build and freeze a one-shot RMTFS diagnostic that stops before SCM."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
PRIVATE = ROOT / '.cache/sunfish-native-wifi/private'
INPUTS_DEFAULT = PRIVATE / 'prepared-boot-inputs-20261025'
BUILD_DEFAULT = PRIVATE / 'native-startup-mpss-runtime-prescm-diagnostic-v6'
FROZEN_DEFAULT = PRIVATE / 'native-mpss-runtime-prescm-frozen-v6'
SOURCES = [
    'rust/Cargo.toml', 'rust/src/sunfish_linux_rmtfs.rs',
    'rust/src/pixel_linux_rmtfs.rs', 'rust/src/sunfish_native_startup_preflight.rs',
    'platforms/sunfish/build-native-startup-preflight.sh',
    'platforms/sunfish/audit-native-mpss-runtime-prescm-diagnostic.py',
    'platforms/sunfish/build-native-mpss-runtime-prescm-diagnostic.py',
    'platforms/sunfish/native-startup-mpss-runtime.system',
]


def require(ok, message):
    if not ok:
        raise SystemExit(message)


def identity(path):
    path = Path(path)
    require(path.is_file(), f'missing build input: {path}')
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return {'bytes': path.stat().st_size, 'sha256': digest.hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', type=Path, default=INPUTS_DEFAULT)
    parser.add_argument('--build-dir', type=Path, default=BUILD_DEFAULT)
    parser.add_argument('--output', type=Path, default=FROZEN_DEFAULT)
    args = parser.parse_args()
    os.umask(0o077)
    inputs = args.inputs.resolve()
    build_dir = args.build_dir.resolve()
    output = args.output.resolve()
    require(inputs.is_dir(), f'prepared private input directory is missing: {inputs}')
    require(not build_dir.exists(), f'refusing to reuse build directory: {build_dir}')
    require(not output.exists(), f'refusing to overwrite frozen output: {output}')
    build_dir.mkdir(parents=True, mode=0o700)
    require(build_dir.stat().st_mode & 0o077 == 0, 'private build directory required')

    audit_path = build_dir / 'pre-scm-source-audit.json'
    subprocess.run([
        sys.executable, str(ROOT / 'platforms/sunfish/audit-native-mpss-runtime-prescm-diagnostic.py'),
        '--workspace', str(ROOT), '--output', str(audit_path),
    ], cwd=ROOT, check=True)

    env = os.environ.copy()
    env.update({
        'SUNFISH_NATIVE_OUTPUT_DIR': str(build_dir),
        'SUNFISH_PRIVATE_INPUT_DIR': str(inputs),
        'SUNFISH_SKIP_NATIVE_TESTS': '1',
    })
    build_log = build_dir / 'build.log'
    command = ['bash', 'platforms/sunfish/build-native-startup-preflight.sh',
               'mpss-runtime-prescm-diagnostic']
    with build_log.open('w') as stream:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=stream,
                                stderr=subprocess.STDOUT, text=True)
    if result.returncode:
        print('\n'.join(build_log.read_text(errors='replace').splitlines()[-60:]), file=sys.stderr)
        raise SystemExit(f'pre-SCM image build failed with status {result.returncode}')

    image = build_dir / 'fastboot.img'
    image_meta = identity(image)
    with image.open('rb') as stream:
        header = stream.read(44)
    require(header[:8] == b'ANDROID!' and int.from_bytes(header[40:44], 'little') == 2,
            'output is not an Android v2 boot image')
    require(image_meta['bytes'] <= 0x10000000, 'temporary image exceeds bounded Fastboot size')
    capdl_path = build_dir / 'capability-audit.json'
    require(capdl_path.is_file(), 'runtime CapDL audit output is missing')
    capdl = json.loads(capdl_path.read_text())
    require(capdl.get('device_actions') == [] and capdl.get('native_wifi_proven') is False,
            'CapDL report claims device actions or Wi-Fi success')
    require(capdl.get('glink_started') is False
            and capdl.get('physical_authentication_proven') is False
            and capdl.get('physical_glink_ready_proven') is False,
            'CapDL report claims a post-auth or GLINK device result')

    output.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    output.mkdir(mode=0o700)
    require(output.stat().st_mode & 0o077 == 0, 'private frozen output required')
    shutil.copyfile(image, output / 'fastboot.img')
    shutil.copyfile(capdl_path, output / 'capability-audit.json')
    shutil.copyfile(audit_path, output / 'source-audit.json')
    shutil.copyfile(build_log, output / 'build.log')
    shutil.copyfile(build_dir / 'report.txt', output / 'report.txt')

    source_identities = {rel: identity(ROOT / rel) for rel in SOURCES}
    input_identities = {
        str(path): identity(path)
        for path in (inputs / 'private_boot_inputs.rs', inputs / 'modemst1.bin',
                     inputs / 'modemst2.bin', inputs / 'wlanmdsp.mbn',
                     inputs / 'fsc.bin', inputs / 'fsg.bin')
    }
    manifest = {
        'created_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'product': 'sunfish',
        'feature': 'sunfish-native-startup-mpss-runtime-prescm-diagnostic',
        'scope': 'Temporary RMTFS pre-assignment checkpoint image. It runs the existing IPA startup, validates RMTFS owners, constructs and stages the assignment bytes, then stops before RMTFS assignment SCM invocation or ownership transfer. Earlier IPA secure-loader calls remain in scope.',
        'stop_boundary': 'READY BEFORE SCM',
        'artifacts': {
            'fastboot.img': identity(output / 'fastboot.img'),
            'capability-audit.json': identity(output / 'capability-audit.json'),
            'source-audit.json': identity(output / 'source-audit.json'),
            'report.txt': identity(output / 'report.txt'),
        },
        'source_sha256': {path: record['sha256'] for path, record in source_identities.items()},
        'input_identities': input_identities,
        'device_actions': [],
        'rmtfs_assignment_scm_invoked': False,
        'ownership_transfer_attempted': False,
        'pas4_attempted': False,
        'reset_released': False,
        'glink_started': False,
        'qrtr_started': False,
        'wlan_started': False,
        'mpss_firmware_transferred': False,
        'wlan_firmware_transferred': False,
        'ipa_firmware_startup_included': True,
        'ipa_secure_loader_calls_may_occur': True,
        'flash_or_retry': False,
        'tests_invoked': False,
        'ready_for_device_review': True,
        'ready_for_device_boot': False,
        'attempt_consumed': False,
        'next_required': 'Review this new image hash and scope, then explicitly approve one cold Fastboot temporary boot if desired. The consumed v6 image is not reused.',
    }
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print('FROZEN:', manifest['artifacts']['fastboot.img']['bytes'], 'bytes, SHA256',
          manifest['artifacts']['fastboot.img']['sha256'])
    print('SCOPE: IPA startup retained; checkpoint-only before RMTFS assignment SCM/PAS4/GLINK/QRTR/WLAN; no device boot')
    print('MANIFEST:', output / 'manifest.json')


if __name__ == '__main__':
    main()
