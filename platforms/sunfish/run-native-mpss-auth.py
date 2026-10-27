#!/usr/bin/env python3
"""Verify or consume one frozen MPSS-auth experiment. Never flash or retry.

Default operation is offline verification only. Execution also requires a
separate record of the user's cold-power-off confirmation; enumeration alone
does not establish that physical precondition. A successful fastboot command
is not a native boot, MPSS-ready, WLAN or network-traffic result.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
PRIVATE = ROOT / '.cache/sunfish-native-wifi/private'
SERIAL = '02281FQC200058'
PRODUCT = 'sunfish'
FEATURE = 'sunfish-native-startup-mpss-auth'
SOURCE_DIAGNOSTIC_FEATURE = 'sunfish-native-startup-mpss-auth-source-diagnostic'


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def require(value, message):
    if not value:
        raise ValueError(message)


def identity(path):
    require(path.is_file(), 'missing file: ' + str(path))
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return dict(bytes=path.stat().st_size, sha256=digest.hexdigest())


def read(path):
    return json.loads(path.read_text())


def relative(root, name):
    lexical = Path(name)
    root = root.resolve()
    require(not lexical.is_absolute() and '..' not in lexical.parts,
            'path escapes reviewed root: ' + name)
    candidate = root / lexical
    path = candidate.resolve()
    if not path.is_relative_to(root):
        # This checkout intentionally symlinks some cache inputs to the
        # sibling main checkout. Do not treat arbitrary .cache symlinks as
        # trusted: the resolved target must remain under that checkout's cache.
        require(lexical.parts and lexical.parts[0] == '.cache'
                and root.parent.name == '.worktrees',
                'path escapes reviewed root: ' + name)
        shared_cache = (root.parent.parent / '.cache').resolve()
        require(path.is_relative_to(shared_cache), 'cache path escapes reviewed cache: ' + name)
    return path


def checked(path, expected):
    actual = identity(path)
    require(actual == expected, 'changed file: ' + str(path))


def verify(out, root=ROOT):
    out = out.resolve()
    require(not out.stat().st_mode & 0o077, 'private experiment directory required')
    require(not (out / 'attempt.json').exists(), 'attempt already consumed; no retry')
    manifest = read(out / 'manifest.json')
    feature = manifest.get('feature')
    require((feature in (FEATURE, SOURCE_DIAGNOSTIC_FEATURE)
             and manifest['serial'] == SERIAL and manifest['product'] == PRODUCT),
            'wrong experiment, device or feature')
    diagnostic_only = feature == SOURCE_DIAGNOSTIC_FEATURE
    require(manifest.get('diagnostic_only', False) is diagnostic_only,
            'feature and diagnostic-only scope disagree')
    if diagnostic_only:
        require(manifest.get('source_guard_stop_before_relocation') is True
                and 'source guard diagnostic only' in manifest.get('scope', '').lower()
                and 'before relocation' in manifest.get('scope', '').lower(),
                'source diagnostic must be explicitly limited to the pre-relocation guard')
    else:
        require(manifest.get('source_guard_stop_before_relocation', False) is False,
                'auth image may not claim the source-diagnostic stop scope')
    require(manifest['requires_full_poweroff_seconds_at_least'] >= 10
            and manifest['ready_for_cold_device_check']
            and not manifest['attempt_consumed'] and not manifest['fastboot_attempted']
            and not manifest['device_actions'], 'unavailable or previously attempted experiment')
    required = {'fastboot.img', 'capability-audit.json', 'boot-layout-audit.json',
                'mpss-auth-source-audit.json', 'native_startup_preflight.elf',
                'loader-sdk-profile.json', 'report.txt', 'capdl.json', 'cargo-build.log'}
    require(required <= manifest['artifacts'].keys(), 'missing required frozen artifacts')
    for name, expected in manifest['artifacts'].items():
        checked(relative(out, name), expected)
    require(manifest['source_sha256'] and manifest['compiler_dependencies']
            and manifest['environment_identities'], 'missing frozen input closures')
    for name, expected in manifest['source_sha256'].items():
        require(identity(relative(root, name))['sha256'] == expected, 'changed reviewed source: ' + name)
        require(identity(relative(out / 'source', name))['sha256'] == expected, 'changed source snapshot: ' + name)
    for name, expected in manifest['compiler_dependencies'].items():
        checked(relative(root, name), expected)
        checked(relative(out / 'compiler-inputs', name), expected)
    for name, expected in manifest['environment_identities'].items():
        checked(Path(name), expected)
    for name, expected in manifest['private_input_identities'].items():
        checked(relative(out / 'private-inputs', name), expected)
    for record in ('capability-audit.json', 'boot-layout-audit.json'):
        for name, expected in read(out / record)['input_sha256'].items():
            require(identity(Path(name))['sha256'] == expected, 'changed image-audit input: ' + name)
    proofs = manifest.get('proofs')
    require(isinstance(proofs, dict) and proofs, 'missing frozen proof identities')
    for name, proof in proofs.items():
        require(Path(name).name == name and name not in ('.', '..'), 'invalid frozen proof name')
        source_lexical = Path(proof['path'])
        source = source_lexical.resolve()
        require(source_lexical.is_absolute() and source.is_relative_to(root.resolve()),
                'proof path escapes reviewed root: ' + str(source_lexical))
        if root.resolve() == ROOT.resolve():
            require(source_lexical.is_relative_to(PRIVATE),
                    'proof path escapes private Sunfish cache: ' + str(source_lexical))
            if not source.is_relative_to(root.resolve()):
                shared_cache = (root.resolve().parent.parent / '.cache').resolve()
                require(root.resolve().parent.name == '.worktrees'
                        and source.is_relative_to(shared_cache),
                        'proof path escapes reviewed shared cache: ' + str(source))
        expected = {'bytes': proof['bytes'], 'sha256': proof['sha256']}
        checked(source, expected)
        checked(out / 'proofs' / name / source.name, expected)
    bound_documents = manifest.get('proof_bound_documents')
    require(isinstance(bound_documents, dict), 'missing proof-bound document closure')
    for key, record in bound_documents.items():
        require('/' in key, 'invalid proof-bound document key: ' + key)
        proof_name, source_name = key.split('/', 1)
        require(Path(proof_name).name == proof_name, 'invalid proof-bound document proof name')
        source_name = record.get('path', source_name)
        current = relative(root, source_name)
        require(identity(current)['sha256'] == record['current_sha256'],
                'changed proof-bound source document: ' + source_name)
        snapshot = relative(out, record['snapshot'])
        require(identity(snapshot)['sha256'] == record['sha256'],
                'changed frozen proof-bound document: ' + key)
    notes = manifest.get('current_notes')
    require(isinstance(notes, dict), 'missing current notes closure')
    note_source = Path(notes['path']).resolve()
    require(note_source.is_absolute() and note_source.is_relative_to(root.resolve()),
            'current notes path escapes reviewed root')
    checked(note_source, {'bytes': notes['bytes'], 'sha256': notes['sha256']})
    checked(relative(out, notes['snapshot']), {'bytes': notes['bytes'], 'sha256': notes['sha256']})
    if diagnostic_only:
        layout = read(out / 'boot-layout-audit.json')
        source_audit = layout.get('source_guard_diagnostic_audit')
        profile = read(out / 'loader-sdk-profile.json')
        require(profile.get('native_source_diagnostic') is True
                and isinstance(source_audit, dict)
                and source_audit.get('all_paths_stop_before_relocation') is True
                and source_audit.get('diagnostic_calls') == 6,
                'frozen image lacks the audited pre-relocation diagnostic stop')
    image = out / 'fastboot.img'
    with image.open('rb') as stream:
        header = stream.read(44)
    require(header[:8] == b'ANDROID!' and int.from_bytes(header[40:44], 'little') == 2,
            'expected the frozen Android-v2 temporary boot image')
    require(identity(image)['bytes'] <= 0x10000000, 'image exceeds bound device download limit')
    return manifest


def confirmation(path, manifest):
    record = read(path)
    require(record.get('source') == 'user_reply' and record.get('user_text')
            and record.get('entered_fastboot_after_poweroff') is True
            and record.get('poweroff_seconds_at_least', 0) >= 10
            and record.get('serial') == SERIAL
            and record.get('image_sha256') == manifest['artifacts']['fastboot.img']['sha256'],
            'a user cold-power-off confirmation for this device and image is required')
    require(record.get('recorded_utc'), 'missing confirmation timestamp')
    return record


def command(args, timeout, log):
    result = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    log.write_text(result.stdout + result.stderr)
    return result.returncode, result.stdout + result.stderr


def variable(text, name):
    matches = re.findall(r'^(?:\(bootloader\)\s*)?' + re.escape(name) + r':\s*(\S+)\s*$', text, re.M)
    require(len(matches) == 1, 'ambiguous/missing fastboot variable: ' + name)
    return matches[0]


def device(fastboot, manifest, run, invoke=command):
    status, text = invoke([fastboot, 'devices', '-l'], 10, run / 'fastboot-devices.log')
    devices = [line.split()[0] for line in text.splitlines() if line.strip()]
    require(status == 0 and devices.count(SERIAL) == 1, 'bound Pixel not enumerated in Fastboot')
    values = {}
    for name in ('product', 'unlocked', 'current-slot', 'max-download-size'):
        status, text = invoke([fastboot, '-s', SERIAL, 'getvar', name], 10, run / ('getvar-' + name + '.log'))
        require(status == 0, 'failed Fastboot identity query: ' + name)
        values[name] = variable(text, name)
    require(values['product'] == PRODUCT and values['unlocked'] in ('yes', 'true'), 'wrong product or locked bootloader')
    require(values['current-slot'] == 'a', 'boot slot changed from the reviewed boot_a environment')
    require(int(values['max-download-size'], 0) == 0x10000000
            and manifest['artifacts']['fastboot.img']['bytes'] <= int(values['max-download-size'], 0),
            'changed or insufficient device download limit')
    return values


def write(path, value, exclusive=False):
    flags = os.O_WRONLY | os.O_CREAT | (os.O_EXCL if exclusive else os.O_TRUNC)
    fd = os.open(path, flags, 0o600)
    with os.fdopen(fd, 'w') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def update(path, value):
    fd, temporary = tempfile.mkstemp(prefix='.receipt-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            json.dump(value, stream, indent=2)
            stream.write('\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def execute(out, cold_path, runs, fastboot, root=ROOT, invoke=command):
    # Verification and identity failures make no boot attempt. A competing
    # runner can win only the exclusive claim; the loser never sends boot.
    out = out.resolve()
    runner_identity = identity(Path(__file__))
    manifest = verify(out, root)
    cold = confirmation(cold_path, manifest)
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    prefix = 'source-guard-diagnostic-' if manifest['feature'] == SOURCE_DIAGNOSTIC_FEATURE else 'mpss-auth-'
    run = runs / (prefix + stamp)
    run.mkdir(parents=True, mode=0o700)
    require(not run.stat().st_mode & 0o077, 'private device record required')
    values = device(fastboot, manifest, run, invoke)
    image = out / 'fastboot.img'
    checked(image, manifest['artifacts']['fastboot.img'])
    archive = run / 'artifacts'
    archive.mkdir(mode=0o700)
    for name in ['manifest.json', *sorted(manifest['artifacts'].keys() & {
            'fastboot.img', 'capability-audit.json', 'boot-layout-audit.json',
            'mpss-auth-source-audit.json', 'capdl.json', 'report.txt',
            'cargo-build.log', 'loader-sdk-profile.json', 'toolchain-versions.json'})]:
        shutil.copyfile(out / name, archive / name)
    for name in ('source', 'proofs', 'notes'):
        if (out / name).is_dir():
            shutil.copytree(out / name, archive / name)
    image = archive / 'fastboot.img'
    checked(image, manifest['artifacts']['fastboot.img'])
    image.chmod(0o400)
    for name, expected in manifest['source_sha256'].items():
        require(identity(relative(archive / 'source', name))['sha256'] == expected,
                'changed archived source: ' + name)
    checked(Path(__file__), runner_identity)
    shutil.copyfile(Path(__file__), run / Path(__file__).name)
    write(run / 'cold-confirmation.json', cold, exclusive=True)
    args = [fastboot, '-s', SERIAL, 'boot', str(image)]
    record = dict(consumed_utc=now(), status='consumed_before_fastboot_boot', serial=SERIAL,
                  product=PRODUCT, image=str(image), frozen_image=str(out / 'fastboot.img'),
                  image_identity=manifest['artifacts']['fastboot.img'],
                  run=str(run), user_cold_fastboot_ready=True, minimum_poweroff_seconds=10,
                  cold_confirmation_sha256=identity(run / 'cold-confirmation.json')['sha256'],
                  runner_sha256=runner_identity['sha256'], device=values,
                  feature=manifest['feature'], diagnostic_only=manifest.get('diagnostic_only', False),
                  scope=manifest['scope'], command=args, automatic_retry_allowed=False,
                  native_mpss_started=False, native_wifi_proven=False)
    write(out / 'attempt.json', record, exclusive=True)
    write(run / 'attempt.json', record, exclusive=True)
    print('CONSUMED before boot:', run, flush=True)
    try:
        code, _ = invoke(args, 120, run / 'fastboot-boot.log')
        record.update(status='fastboot_command_completed' if code == 0 else 'fastboot_command_failed',
                      finished_utc=now(), returncode=code)
    except BaseException as error:
        record.update(status='fastboot_outcome_uncertain', finished_utc=now(), error=type(error).__name__)
        update(out / 'attempt.json', record)
        update(run / 'attempt.json', record)
        raise
    update(out / 'attempt.json', record)
    update(run / 'attempt.json', record)
    print(record['status'], '; attempt remains consumed; capture the device screen.', flush=True)
    return code, run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image-dir', type=Path, required=True)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--cold-confirmation', type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    out = args.image_dir.resolve()
    require(out.is_relative_to(PRIVATE), 'use the private frozen experiment directory')
    if not args.execute:
        manifest = verify(out)
        print('VERIFIED frozen image:', manifest['artifacts']['fastboot.img']['sha256'], '; no device action.')
        return 0
    require(args.cold_confirmation is not None, '--execute requires the recorded user cold confirmation')
    fastboot = shutil.which('fastboot')
    require(fastboot is not None, 'fastboot executable unavailable')
    code, _ = execute(out, args.cold_confirmation, PRIVATE / 'device-runs', fastboot)
    return code


if __name__ == '__main__':
    sys.exit(main())
