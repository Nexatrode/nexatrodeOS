#!/usr/bin/env python3
"""Collect and audit the private post-auth GLINK/QRTR integration build.

All operations are offline. This creates source, test, CapDL, and loader
evidence for the separate freezer; it performs no Fastboot or device action.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
PRIVATE = ROOT / '.cache/sunfish-native-wifi/private'
BUILD = PRIVATE / 'native-startup-mpss-runtime'
OUTPUT = PRIVATE / 'mpss-runtime-review-final'
PREDECESSOR = PRIVATE / 'device-runs/mpss-auth-20261027T192834885146Z/artifacts'
EXPECTED_CNODE = [1, 2, 3, 4, 7, *range(138, 145)]
EXPECTED_IRQS = {'132': 0, '97': 1, '343': 2, '464': 3, '481': 4, '483': 5, '298': 6}


def require(value, message):
    if not value:
        raise ValueError(message)


def identity(path):
    path = Path(path)
    require(path.is_file(), 'missing integration input: ' + str(path))
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return {'bytes': path.stat().st_size, 'sha256': digest.hexdigest()}


def relative(path):
    path = Path(path).resolve()
    return str(path.relative_to(ROOT))


def copy_private(source, destination):
    destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    destination.chmod(0o600)


def run(command, log):
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    log.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    log.write_text(result.stdout + result.stderr)
    log.chmod(0o600)
    require(result.returncode == 0, 'offline command failed: ' + ' '.join(command)
            + '; see ' + str(log))
    return result.stdout + result.stderr


def dependency_paths(depfile):
    content = Path(depfile).read_text().replace('\\\n', ' ')
    require(':' in content, 'malformed Rust compiler dependency file: ' + str(depfile))
    paths = set()
    for name in shlex.split(content.split(':', 1)[1]):
        path = Path(name)
        if not path.is_absolute():
            path = ROOT / path
        path = path.resolve()
        require(path.is_file(), 'missing recorded compiler input: ' + str(path))
        paths.add(path)
    return paths


def load_json(path):
    return json.loads(Path(path).read_text())


def verify_predecessor():
    result_path = PREDECESSOR / 'settled-screen-result.json'
    photo_path = PREDECESSOR / 'settled-screen.heic'
    result = load_json(result_path)
    evidence = result['screen_evidence']
    require(evidence.get('source') == 'user-supplied photo'
            and identity(photo_path)['sha256'] == evidence.get('sha256'),
            'predecessor auth photo hash/source mismatch')
    assessment = result['assessment']
    readout = result['readout']
    require(assessment.get('pas4_return_observed') is True
            and assessment.get('delivered_q6_ready_observed') is True
            and assessment.get('glink_open_proven') is False
            and assessment.get('wlan_firmware_or_association_proven') is False
            and assessment.get('network_frames_proven') is False,
            'predecessor evidence does not resolve PAS4 and delivered Q6-ready')
    require(readout.get('phase_text') == 'MPSS AUTH READY PASS'
            and readout.get('loader_phase_name') == 'AuthReturned'
            and readout.get('auth_receipt') is True and readout.get('ready_receipt') is True
            and readout.get('glink_started') is False and readout.get('wlan_started') is False
            and readout.get('flash_or_reset_retry') is False,
            'predecessor settled screen is not the reviewed PAS4/Q6-ready result')
    return result_path, photo_path, result


def source_paths(source_audit):
    paths = set(source_audit['source_sha256'])
    paths.update({
        'rust/Cargo.lock',
        'rust/targets/aarch64-sel4-microkit-pixel4k.json',
        'platforms/sunfish/audit-native-mpss-runtime-layout.py',
        'platforms/sunfish/audit-native-mpss-runtime-source.py',
        'platforms/sunfish/build-native-mpss-runtime-integration.py',
        'platforms/sunfish/freeze-native-mpss-runtime.py',
        'tests/test-native-mpss-runtime-freezer.py',
        'platforms/sunfish/test-native-mpss-runtime-irq.py',
        'platforms/sunfish/test-native-mpss-runtime-services.py',
        'platforms/sunfish/test-native-mpss-runtime-source.py',
        'platforms/sunfish/test-native-mpss-auth-root.py',
        'platforms/sunfish/test-native-mpss-linux-transport.py',
        'platforms/sunfish/test-native-linux-rmtfs-server.py',
        'platforms/sunfish/native-startup-mpss-auth.system',
        'docs/superpowers/specs/2026-10-27-sunfish-post-auth-glink-runtime-design.md',
        'docs/superpowers/plans/2026-10-27-sunfish-post-auth-glink-runtime.md',
        '.superpowers/sdd/2026-10-27-sunfish-post-auth-glink-runtime/progress.md',
        'platforms/sunfish/README.md',
    })
    return sorted(paths)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-dir', type=Path, default=BUILD)
    parser.add_argument('--output-dir', type=Path, default=OUTPUT)
    parser.add_argument('--private-inputs', type=Path,
                        default=ROOT / '.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025')
    parser.add_argument('--reference-boot', type=Path,
                        default=Path.home() / 'Backups/pixel-sunfish-20261006/live-slots/boot_a.img')
    parser.add_argument('--gate-diagnostic', action='store_true',
                        help='bind the image to the diagnostic path that stops before PAS4')
    args = parser.parse_args()
    os.umask(0o077)
    build_dir, output = args.build_dir.resolve(), args.output_dir.resolve()
    require(build_dir.stat().st_mode & 0o077 == 0, 'private completed preflight directory required')
    build_profile = load_json(build_dir / 'build-profile.json')
    expected_mode = 'mpss-runtime-gate-diagnostic' if args.gate_diagnostic else 'mpss-runtime'
    expected_feature = ('sunfish-native-startup-mpss-runtime-gate-diagnostic'
                        if args.gate_diagnostic else 'sunfish-native-startup-mpss-runtime')
    require(build_profile.get('mode') == expected_mode
            and build_profile.get('feature') == expected_feature
            and build_profile.get('gate_diagnostic') is args.gate_diagnostic
            and build_profile.get('loader_source_guard') == 'labeled-pass-through'
            and build_profile.get('device_actions') == [],
            'build profile does not match requested runtime mode')
    require(not output.exists(), 'integration proof directory already exists; refusing overwrite')
    runtime_system = ROOT / 'platforms/sunfish/native-startup-mpss-runtime.system'
    packaged_system = build_dir / runtime_system.name
    if packaged_system.exists():
        require(identity(packaged_system) == identity(runtime_system),
                'preflight runtime system differs from reviewed source')
    else:
        copy_private(runtime_system, packaged_system)
    required = [
        'fastboot.img', 'fastboot.img.sha256', 'native_startup_preflight.elf',
        'loader.img', 'capdl.json', 'report.txt', 'cargo-build.log',
        'build-profile.json',
        'native-startup-mpss-runtime.system',
        'loader-sdk-profile.json', 'capability-audit.json', 'runtime-source-audit.json',
        'boot-layout-audit.json', 'mpss-runtime-service-tests.log',
        'mpss-runtime-irq-tests.log', 'mpss-runtime-source-tests.log',
        'mpss-auth-root-tests.log', 'mpss-boot-tests.log',
    ]
    for name in required:
        identity(build_dir / name)

    freezer_test_log = build_dir / 'runtime-freezer-tests.log'
    freezer_test_output = run(['python3', 'tests/test-native-mpss-runtime-freezer.py'], freezer_test_log)
    require('Ran 6 tests' in freezer_test_output and 'OK' in freezer_test_output,
            'freeze regression suite did not pass')
    runtime_source_test_log = build_dir / 'mpss-runtime-source-tests.log'
    runtime_source_test_output = run(
        ['python3', 'platforms/sunfish/test-native-mpss-runtime-source.py'], runtime_source_test_log)
    source_count = re.search(r'Ran (\d+) tests', runtime_source_test_output)
    require(source_count is not None and int(source_count.group(1)) >= 5
            and 'OK' in runtime_source_test_output,
            'runtime source regression suite did not pass')
    runtime_irq_log = build_dir / 'mpss-runtime-irq-tests.log'
    runtime_irq_output = run(['python3', 'platforms/sunfish/test-native-mpss-runtime-irq.py'], runtime_irq_log)
    require('PASS: 13 router groups' in runtime_irq_output and 'no device action' in runtime_irq_output,
            'runtime IRQ/AArch64 check did not pass')
    source_audit_path = build_dir / 'runtime-source-audit.json'
    run(['python3', 'platforms/sunfish/audit-native-mpss-runtime-source.py',
         '--workspace', str(ROOT), '--output', str(source_audit_path)],
        build_dir / 'runtime-source-audit-tests.log')

    source_audit = load_json(source_audit_path)
    capability = load_json(build_dir / 'capability-audit.json')
    boot_layout = load_json(build_dir / 'boot-layout-audit.json')
    require(source_audit.get('result') == 'passed' and source_audit.get('device_actions') == [],
            'static runtime source audit failed')
    require(capability.get('mutations_rejected', 0) >= 276
            and capability.get('glink_irq') == 481 and capability.get('glink_channel') == 4
            and capability.get('device_actions') == [] and capability.get('native_wifi_proven') is False,
            'runtime CapDL audit failed or exceeds scope')
    require('Finished `release` profile' in (build_dir / 'cargo-build.log').read_text(),
            'completed AArch64 cargo build not found')
    compiled = ROOT / '.cache/sunfish-native-wifi/private/preflight-target/aarch64-sel4-microkit-pixel4k/release'
    root_elf = compiled / 'rust_sunfish_native_startup_preflight.elf'
    require(identity(root_elf) == identity(build_dir / 'native_startup_preflight.elf'),
            'copied AArch64 root ELF differs from the compiled binary')
    cargo_dep = compiled / 'rust_sunfish_native_startup_preflight.d'
    lib_dep = compiled / 'libpersonal_os_rust_server.d'
    dependencies = sorted(dependency_paths(cargo_dep) | dependency_paths(lib_dep))
    compiler_dependencies = {str(path): identity(path) for path in dependencies}

    transport = load_json(PRIVATE / 'mpss-linux-transport/result.json')
    auth_root = load_json(PRIVATE / 'mpss-auth-root/result.json')
    runtime_irq = load_json(PRIVATE / 'mpss-runtime-irq/result.json')
    require(transport.get('result') == 'passed' and transport.get('transport_groups') == 18
            and len(transport.get('mutations_rejected', [])) >= 10
            and transport.get('device_actions') == [] and transport.get('native_wifi_proven') is False,
            'GLINK transport regression result is stale or incomplete')
    require(auth_root.get('root_groups') == 10 and auth_root.get('device_actions') == [],
            'auth root lifecycle regression proof is missing')
    require(runtime_irq.get('result') == 'passed'
            and runtime_irq.get('linux_startup_order') == {
                'pas4_before_q6_ready': True,
                'remoteproc_subdevices_after_start_callback': True,
                'glink_subdevice_registers_smem': True,
            }
            and len(runtime_irq.get('linux_startup_order_mutations_rejected', [])) == 3
            and runtime_irq.get('device_actions') == [],
            'pinned PAS/Q6/GLINK source-order proof is missing')

    services_log = (build_dir / 'mpss-runtime-service-tests.log').read_text()
    service_counts = re.search(r'PASS: (\d+) GLINK/QMI cases and (\d+) owner/RMTFS cases', services_log)
    require(service_counts is not None and int(service_counts.group(1)) >= 148
            and int(service_counts.group(2)) >= 210,
            'integrated runtime service suite did not pass')
    root_log = (build_dir / 'mpss-auth-root-tests.log').read_text()
    require('PASS: 10 root groups, 281 total host groups' in root_log,
            'runtime receipt and gate tests did not pass')

    previous_path, photo_path, previous = verify_predecessor()
    previous_identity = identity(previous_path)
    photo_identity = identity(photo_path)
    evidence = {
        'source_audit': source_audit,
        'capability_audit': capability,
        'layout_audit': {},
        'diagnostic_only': args.gate_diagnostic,
        'test_results': {
            'runtime_services': True,
            'runtime_irq': True,
            'runtime_source': True,
            'auth_root': True,
            'transport': True,
            'aarch64_build': True,
            'capdl': True,
            'runtime_layout': False,
            'freezer_tests': True,
        },
        'previous_auth': previous,
    }
    # The layout audit consumes this receipt and binds its exact root ELF and
    # the already reviewed predecessor photo before any loader-range claims.
    integration_receipt = {
        'result': 'passed', 'root_elf_sha256': identity(build_dir / 'native_startup_preflight.elf')['sha256'],
        'predecessor_auth_evidence_bound': True,
        'predecessor_auth_result_sha256': previous_identity['sha256'],
        'predecessor_auth_photo_sha256': photo_identity['sha256'],
        'device_actions': [], 'native_wifi_proven': False,
    }
    (build_dir / 'integration-result.json').write_text(json.dumps(integration_receipt, indent=2) + '\n')
    layout_log = build_dir / 'runtime-layout-audit.log'
    layout_output = run(['python3', 'platforms/sunfish/audit-native-mpss-runtime-layout.py',
                         '--output-dir', str(build_dir), '--loader-reference', str(build_dir)], layout_log)
    require('PASS: actual runtime loader/copy/frame ranges' in layout_output,
            'runtime loader-range audit did not pass')
    layout = load_json(build_dir / 'runtime-loader-layout-audit.json')
    evidence['layout_audit'] = layout
    evidence['test_results']['runtime_layout'] = True
    evidence['test_results']['freezer_tests'] = True

    # Recheck the exact Android-v2 image wrapper before its later separate freeze.
    image_path = build_dir / 'fastboot.img'
    with image_path.open('rb') as stream:
        header = stream.read(44)
    require(header[:8] == b'ANDROID!' and int.from_bytes(header[40:44], 'little') == 2
            and identity(image_path)['bytes'] <= 0x10000000,
            'expected bounded temporary Android-v2 boot image')
    recorded_hash = (build_dir / 'fastboot.img.sha256').read_text().split()[0]
    require(recorded_hash == identity(image_path)['sha256'], 'Fastboot image SHA file mismatch')

    # Import the same pure gate used by the freezer before producing snapshots.
    spec_path = ROOT / 'platforms/sunfish/freeze-native-mpss-runtime.py'
    import importlib.util
    spec = importlib.util.spec_from_file_location('runtime_freezer', spec_path)
    freezer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(freezer)
    freezer.validate_evidence(evidence, gate_diagnostic=args.gate_diagnostic)

    output.mkdir(mode=0o700, parents=True)
    require(output.stat().st_mode & 0o077 == 0, 'private integration output required')
    (output / 'source').mkdir(mode=0o700)
    (output / 'proofs').mkdir(mode=0o700)
    (output / 'artifacts').mkdir(mode=0o700)

    source_hashes = {}
    for rel in source_paths(source_audit):
        path = ROOT / rel
        record = identity(path)
        if rel in source_audit['source_sha256']:
            require(record['sha256'] == source_audit['source_sha256'][rel],
                    'source audit hash changed before bundle: ' + rel)
        source_hashes[rel] = record['sha256']
        copy_private(path, output / 'source' / rel)

    proof_sources = {
        'predecessor-auth-result.json': previous_path,
        'predecessor-auth-screen.heic': photo_path,
        'runtime-services.log': build_dir / 'mpss-runtime-service-tests.log',
        'runtime-irq.log': runtime_irq_log,
        'runtime-source-tests.log': runtime_source_test_log,
        'runtime-source-audit.json': source_audit_path,
        'runtime-layout-audit.log': layout_log,
        'auth-root-result.json': PRIVATE / 'mpss-auth-root/result.json',
        'glink-transport-result.json': PRIVATE / 'mpss-linux-transport/result.json',
        'runtime-irq-result.json': PRIVATE / 'mpss-runtime-irq/result.json',
        'source-order-reference.json': PRIVATE / 'mpss-runtime-irq/result.json',
    }
    for name, path in proof_sources.items():
        copy_private(path, output / 'proofs' / name)
    proof_files = {name: identity(output / 'proofs' / name) for name in proof_sources}

    artifact_names = [
        'capability-audit.json', 'boot-layout-audit.json', 'runtime-source-audit.json',
        'runtime-loader-layout-audit.json', 'native_startup_preflight.elf',
        'capdl.json', 'report.txt', 'cargo-build.log', 'loader-sdk-profile.json',
        'build-profile.json',
        'native-startup-mpss-runtime.system',
        'fastboot.img.sha256', 'mpss-runtime-service-tests.log',
        'mpss-runtime-irq-tests.log', 'mpss-runtime-source-tests.log',
        'mpss-auth-root-tests.log', 'mpss-boot-tests.log',
    ]
    for name in artifact_names:
        copy_private(build_dir / name, output / 'artifacts' / name)
    artifact_records = {name: identity(output / 'artifacts' / name) for name in artifact_names}

    build_paths = {
        'fastboot.img': str(image_path),
        'loader.img': str(build_dir / 'loader.img'),
        'native_startup_preflight.elf': str(build_dir / 'native_startup_preflight.elf'),
        'capdl.json': str(build_dir / 'capdl.json'),
        'report.txt': str(build_dir / 'report.txt'),
        'cargo-build.log': str(build_dir / 'cargo-build.log'),
    }
    build_artifacts = {path: identity(path) for path in build_paths.values()}

    environment_paths = [
        args.reference_boot.resolve(),
        ROOT / '.cache/pixel-sunfish/live-runtime.dtb',
        ROOT / '.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb',
        ROOT / '.cache/sunfish-mobian-comparison/artifacts/sm7150-google-sunfish.dtb',
        ROOT / '.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish/bin/microkit',
        ROOT / '.cache/sunfish-native-wifi/private/native-startup-mpss-runtime/private-modem.mbn',
        ROOT / '.cache/sunfish-native-wifi/private/native-startup-mpss-runtime/private-ipa_fws.mbn',
    ]
    for path in sorted(args.private_inputs.iterdir()):
        if path.is_file():
            environment_paths.append(path.resolve())
    environment = {str(path): identity(path) for path in environment_paths}
    predecessor_identity = {
        'result': previous_identity,
        'photo': photo_identity,
    }

    manifest = {
        'result': 'passed',
        'created_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'feature': build_profile['feature'],
        'build_mode': build_profile['mode'],
        'gate_diagnostic': args.gate_diagnostic,
        'scope': ('Offline runtime gate diagnostic only; always stops before PAS4, reset release, GLINK, or WLAN.'
                  if args.gate_diagnostic else
                  'Offline post-auth GLINK/QRTR service-discovery integration only; WLAN and network traffic are unproven.'),
        'device_actions': [], 'native_wifi_proven': False,
        'evidence': evidence,
        'test_results': evidence['test_results'],
        'source_sha256': source_hashes,
        'source_snapshot': str(output / 'source'),
        'compiler_dependency_sha256': compiler_dependencies,
        'proof_files': proof_files,
        'proof_snapshot': str(output / 'proofs'),
        'artifacts': artifact_records,
        'artifact_snapshot': str(output / 'artifacts'),
        'build_artifacts': build_artifacts,
        'build_paths': build_paths,
        'previous_auth_evidence_identity': predecessor_identity,
        'environment_identities': environment,
        'fastboot_image_sha256': build_artifacts[str(image_path)]['sha256'],
        'capdl_mutations_rejected': capability['mutations_rejected'],
        'loader_range_mutations_rejected': len(layout['mutations_rejected']),
    }
    (output / 'integration-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print('PASS: private source/test/ELF/CapDL integration bundle; '
          + str(len(source_hashes)) + ' source hashes, '
          + str(len(compiler_dependencies)) + ' compiler dependency hashes, '
          + str(capability['mutations_rejected']) + ' CapDL mutations; no device action.')


if __name__ == '__main__':
    main()
