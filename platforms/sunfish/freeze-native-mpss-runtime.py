#!/usr/bin/env python3
"""Freeze an offline-audited, temporary post-auth GLINK/QRTR image.

This tool only copies and hashes local artifacts. It never invokes Fastboot.
The resulting manifest requires a separate review before any device boot.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import shutil

ROOT = Path(__file__).resolve().parents[2]
PRIVATE = ROOT / '.cache/sunfish-native-wifi/private'
EXPECTED_CNODE_SLOTS = [1, 2, 3, 4, 7, *range(138, 145)]
EXPECTED_IRQS = {'132': 0, '97': 1, '343': 2, '464': 3, '481': 4, '483': 5, '298': 6}
EXPECTED_TESTS = {
    'runtime_services', 'runtime_irq', 'runtime_source', 'auth_root',
    'transport', 'aarch64_build', 'capdl', 'runtime_layout',
}


def require(value, message):
    if not value:
        raise ValueError(message)


def identity(path):
    path = Path(path)
    require(path.is_file(), 'missing frozen input: ' + str(path))
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return {'bytes': path.stat().st_size, 'sha256': digest.hexdigest()}


def validate_hash_bindings(expected, actual):
    require(isinstance(expected, dict) and isinstance(actual, dict), 'hash binding maps are required')
    require(expected == actual, 'source or compiler dependency hash binding changed')


def validate_evidence(evidence, gate_diagnostic=False):
    source = evidence['source_audit']
    require(source.get('result') == 'passed' and source.get('device_actions') == [],
            'runtime static source audit did not pass offline')
    require(source.get('native_wifi_proven') is False
            and source.get('tftp_service_published') is False
            and source.get('wlan_firmware_tftp_serving_enabled') is False
            and source.get('ipa_server_endpoint_can_be_announced') is True,
            'source audit exceeds bounded runtime scope')

    cap = evidence['capability_audit']
    require(cap.get('glink_irq') == 481 and cap.get('glink_channel') == 4
            and cap.get('all_native_irq_channels') == EXPECTED_IRQS,
            'GLINK IRQ mapping or retained IRQ routes changed')
    require(cap.get('exact_native_cnode_slots') == EXPECTED_CNODE_SLOTS,
            'CapDL authority differs from the reviewed owner set')
    require(cap.get('added_smc_authority') is False
            and cap.get('added_memory_mapping_authority') is False
            and cap.get('physical_self_notify_authority') is True,
            'runtime CapDL added sensitive authority')
    require(cap.get('rmtfs_owner_retained') is True
            and cap.get('rmtfs_available_before_binding') is False
            and cap.get('tftp_service_published') is False
            and cap.get('wlan_firmware_tftp_serving_enabled') is False
            and cap.get('ipa_server_endpoint_can_be_announced') is True,
            'runtime QRTR service publication policy changed')
    require(cap.get('mutations_rejected', 0) >= 276
            and cap.get('device_actions') == []
            and cap.get('native_wifi_proven') is False
            and cap.get('production_ready') is False,
            'CapDL authority audit incomplete or mis-scoped')

    layout = evidence['layout_audit']
    require(layout.get('all_actual_frames_and_copy_targets_checked') is True
            and 0 < layout.get('loader_bytes', 0) <= 0x6000000
            and layout.get('source_and_destination_envelope_bytes') == 0x6000000
            and layout.get('mutations_rejected')
            and layout.get('device_actions') == []
            and layout.get('native_wifi_proven') is False,
            'runtime loader or CapDL frame-range audit incomplete')
    require(layout.get('predecessor_auth_evidence_bound') is True,
            'predecessor PAS4/Q6-ready device result is not bound')

    tests = evidence['test_results']
    require(EXPECTED_TESTS <= tests.keys() and all(tests[name] is True for name in EXPECTED_TESTS),
            'one or more required host, target, CapDL or layout gates failed')

    previous = evidence['previous_auth']
    assessment = previous.get('assessment', {})
    readout = previous.get('readout', {})
    require(previous.get('screen_evidence', {}).get('source') == 'user-supplied photo'
            and previous.get('screen_evidence', {}).get('sha256')
            and assessment.get('pas4_return_observed') is True
            and assessment.get('delivered_q6_ready_observed') is True
            and assessment.get('glink_open_proven') is False
            and assessment.get('wlan_firmware_or_association_proven') is False
            and assessment.get('network_frames_proven') is False,
            'predecessor PAS4/Q6-ready result is missing or unresolved')
    require(readout.get('phase_text') == 'MPSS AUTH READY PASS'
            and readout.get('loader_phase_name') == 'AuthReturned'
            and readout.get('auth_receipt') is True and readout.get('ready_receipt') is True
            and readout.get('glink_started') is False and readout.get('wlan_started') is False
            and readout.get('flash_or_reset_retry') is False,
            'predecessor screen does not establish the expected retained auth handoff')


def verify_integration(manifest, root=ROOT):
    gate_diagnostic = manifest.get('gate_diagnostic') is True
    expected_mode = ('mpss-runtime-gate-diagnostic' if gate_diagnostic else 'mpss-runtime')
    expected_feature = ('sunfish-native-startup-mpss-runtime-gate-diagnostic'
                        if gate_diagnostic else 'sunfish-native-startup-mpss-runtime')
    require(manifest.get('build_mode') == expected_mode
            and manifest.get('feature') == expected_feature
            and manifest.get('evidence', {}).get('diagnostic_only') is gate_diagnostic,
            'integration mode does not match its diagnostic scope')
    validate_evidence(manifest['evidence'], gate_diagnostic=gate_diagnostic)
    root = Path(root).resolve()
    current_sources = {}
    for rel, expected in manifest['source_sha256'].items():
        lexical = Path(rel)
        require(not lexical.is_absolute() and '..' not in lexical.parts, 'unsafe source path: ' + rel)
        current = identity(root / lexical)
        require(current['sha256'] == expected, 'changed reviewed source: ' + rel)
        current_sources[rel] = current['sha256']
        snapshot = identity(Path(manifest['source_snapshot']) / lexical)
        require(snapshot['sha256'] == expected, 'changed source snapshot: ' + rel)
    validate_hash_bindings(manifest['source_sha256'], current_sources)
    current_dependencies = {}
    for path, expected in manifest['compiler_dependency_sha256'].items():
        current_dependencies[path] = identity(Path(path))
    validate_hash_bindings(manifest['compiler_dependency_sha256'], current_dependencies)
    for name, record in manifest['proof_files'].items():
        require(Path(name).name == name and name not in ('.', '..'), 'unsafe proof file: ' + name)
        require(identity(Path(manifest['proof_snapshot']) / name) == record,
                'changed proof snapshot: ' + name)
    for name, record in manifest['artifacts'].items():
        require(Path(name).name == name and identity(Path(manifest['artifact_snapshot']) / name) == record,
                'changed integration artifact: ' + name)
    for path, record in manifest['build_artifacts'].items():
        require(identity(Path(path)) == record, 'changed preflight build artifact: ' + path)
    current_environment = {}
    for path, record in manifest['environment_identities'].items():
        current_environment[path] = identity(Path(path))
    validate_hash_bindings(manifest['environment_identities'], current_environment)
    require(manifest.get('device_actions') == [] and manifest.get('native_wifi_proven') is False,
            'integration proof claims device action or Wi-Fi success')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--integration', type=Path,
                        default=PRIVATE / 'mpss-runtime-review-final/integration-manifest.json')
    parser.add_argument('--output', type=Path,
                        default=PRIVATE / 'native-mpss-runtime-review')
    args = parser.parse_args()
    os.umask(0o077)
    manifest_path = args.integration.resolve()
    integration_dir = manifest_path.parent
    require(integration_dir.stat().st_mode & 0o077 == 0, 'private integration directory required')
    integration = json.loads(manifest_path.read_text())
    verify_integration(integration)
    output = args.output.resolve()
    require(not output.exists(), 'frozen output already exists; refusing overwrite')
    output.mkdir(parents=True, mode=0o700)
    require(output.stat().st_mode & 0o077 == 0, 'private frozen output required')

    shutil.copytree(integration_dir / 'source', output / 'source')
    shutil.copytree(integration_dir / 'proofs', output / 'proofs')
    shutil.copytree(integration_dir / 'artifacts', output / 'artifacts')
    image_path = Path(integration['build_paths']['fastboot.img'])
    image_record = identity(image_path)
    require(image_record['sha256'] == integration['fastboot_image_sha256']
            and integration['build_artifacts'].get(str(image_path)) == image_record,
            'temporary image is not bound to the reviewed build hash')
    image_header = image_path.read_bytes()[:44]
    require(image_header[:8] == b'ANDROID!' and int.from_bytes(image_header[40:44], 'little') == 2,
            'temporary image is not an Android-v2 boot image')
    require(image_record['bytes'] <= 0x10000000, 'temporary image exceeds the bound download size')
    shutil.copyfile(image_path, output / 'fastboot.img')
    require(identity(output / 'fastboot.img') == image_record, 'copied temporary image hash changed')
    shutil.copyfile(manifest_path, output / 'integration-manifest.json')

    frozen = {
        'created_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'feature': integration['feature'],
        'product': 'sunfish',
        'build_mode': integration['build_mode'],
        'gate_diagnostic': integration['gate_diagnostic'],
        'scope': ('Temporary read-only runtime-gate diagnostic. Reads global/context SMMU status and control, then always stops before PAS4, reset release, GLINK, or WLAN.'
                  if integration['gate_diagnostic'] else
                  'Temporary IPA startup, RMTFS secure share, modem authentication and GLINK/QRTR. The AP IPA service can be announced after its live readiness guard. No TFTP publication, WLAN start, flash, association or network-frame proof.'),
        'artifacts': {
            'fastboot.img': identity(output / 'fastboot.img'),
            'capability-audit.json': identity(output / 'artifacts/capability-audit.json'),
            'runtime-source-audit.json': identity(output / 'artifacts/runtime-source-audit.json'),
            'runtime-loader-layout-audit.json': identity(output / 'artifacts/runtime-loader-layout-audit.json'),
            'native_startup_preflight.elf': identity(output / 'artifacts/native_startup_preflight.elf'),
            'capdl.json': identity(output / 'artifacts/capdl.json'),
            'report.txt': identity(output / 'artifacts/report.txt'),
        },
        'source_sha256': integration['source_sha256'],
        'compiler_dependency_sha256': integration['compiler_dependency_sha256'],
        'environment_identities': integration['environment_identities'],
        'test_results': integration['test_results'],
        'previous_auth_evidence': integration['previous_auth_evidence_identity'],
        'capdl_mutations_rejected': integration['evidence']['capability_audit']['mutations_rejected'],
        'runtime_loader_mutations_rejected': len(integration['evidence']['layout_audit']['mutations_rejected']),
        'tftp_service_published': False,
        'wlan_firmware_tftp_serving_enabled': False,
        'ipa_server_endpoint_can_be_announced': True,
        'native_wifi_proven': False,
        'physical_glink_ready_proven': False,
        'network_frames_proven': False,
        'ready_for_device_review': True,
        'ready_for_device_boot': False,
        'attempt_consumed': False,
        'fastboot_attempted': False,
        'device_actions': [],
        'next_required': ('Review this frozen manifest and temporary image before a one-shot cold Fastboot gate check. It will not run PAS4, release reset, or start GLINK/WLAN.'
                          if integration['gate_diagnostic'] else
                          'Review this frozen manifest and temporary image before deciding whether to run a one-shot cold Fastboot check. A successful boot command alone will not prove GLINK/QRTR discovery or Wi-Fi.'),
    }
    (output / 'manifest.json').write_text(json.dumps(frozen, indent=2) + '\n')
    print('FROZEN:', frozen['artifacts']['fastboot.img']['bytes'], 'bytes, SHA256',
          frozen['artifacts']['fastboot.img']['sha256'], '; QRTR discovery only; no device action.')


if __name__ == '__main__':
    main()
