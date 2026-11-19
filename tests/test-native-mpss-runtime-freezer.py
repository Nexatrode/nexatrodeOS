#!/usr/bin/env python3
"""Regression checks for refusing an incomplete runtime image freeze."""
from pathlib import Path
import importlib.util
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    'native_mpss_runtime_freezer', ROOT / 'platforms/sunfish/freeze-native-mpss-runtime.py')
freezer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(freezer)


def valid_evidence():
    return {
        'source_audit': {
            'result': 'passed', 'device_actions': [], 'native_wifi_proven': False,
            'tftp_service_published': False, 'wlan_firmware_tftp_serving_enabled': False,
            'ipa_server_endpoint_can_be_announced': True,
        },
        'capability_audit': {
            'glink_irq': 481, 'glink_channel': 4,
            'all_native_irq_channels': {'132': 0, '97': 1, '343': 2, '464': 3, '481': 4, '483': 5, '298': 6},
            'exact_native_cnode_slots': [1, 2, 3, 4, 7, *range(138, 145)],
            'added_smc_authority': False, 'added_memory_mapping_authority': False,
            'physical_self_notify_authority': True, 'tftp_service_published': False,
            'wlan_firmware_tftp_serving_enabled': False,
            'ipa_server_endpoint_can_be_announced': True,
            'rmtfs_owner_retained': True, 'rmtfs_available_before_binding': False,
            'native_wifi_proven': False,
            'mutations_rejected': 276, 'production_ready': False, 'device_actions': [],
        },
        'layout_audit': {
            'all_actual_frames_and_copy_targets_checked': True,
            'loader_bytes': 0x100000, 'source_and_destination_envelope_bytes': 0x6000000,
            'mutations_rejected': ['reserved target', 'live frame'],
            'predecessor_auth_evidence_bound': True,
            'native_wifi_proven': False, 'device_actions': [],
        },
        'test_results': {
            'runtime_services': True, 'runtime_irq': True, 'runtime_source': True,
            'auth_root': True, 'transport': True, 'aarch64_build': True,
            'capdl': True, 'runtime_layout': True,
        },
        'previous_auth': {
            'screen_evidence': {'source': 'user-supplied photo', 'sha256': 'evidence'},
            'readout': {
                'phase_text': 'MPSS AUTH READY PASS', 'loader_phase_name': 'AuthReturned',
                'auth_receipt': True, 'ready_receipt': True, 'glink_started': False,
                'wlan_started': False, 'flash_or_reset_retry': False,
            },
            'assessment': {
                'pas4_return_observed': True, 'delivered_q6_ready_observed': True,
                'glink_open_proven': False, 'wlan_firmware_or_association_proven': False,
                'network_frames_proven': False,
            },
        },
    }


class RuntimeFreezer(unittest.TestCase):
    def test_accepts_only_complete_offline_qrtr_scope(self):
        freezer.validate_evidence(valid_evidence())

    def test_rejects_extra_capdl_authority(self):
        evidence = valid_evidence()
        evidence['capability_audit']['exact_native_cnode_slots'].append(255)
        with self.assertRaises(ValueError):
            freezer.validate_evidence(evidence)

    def test_rejects_tftp_or_wlan_firmware_publication(self):
        for field in ('tftp_service_published', 'wlan_firmware_tftp_serving_enabled'):
            evidence = valid_evidence()
            evidence['capability_audit'][field] = True
            with self.subTest(field=field), self.assertRaises(ValueError):
                freezer.validate_evidence(evidence)

    def test_rejects_unbounded_loader_or_frame_audit(self):
        evidence = valid_evidence()
        evidence['layout_audit']['all_actual_frames_and_copy_targets_checked'] = False
        with self.assertRaises(ValueError):
            freezer.validate_evidence(evidence)
        evidence = valid_evidence()
        evidence['layout_audit']['loader_bytes'] = 0x6000001
        with self.assertRaises(ValueError):
            freezer.validate_evidence(evidence)

    def test_rejects_unresolved_predecessor_authentication(self):
        evidence = valid_evidence()
        evidence['previous_auth']['assessment']['delivered_q6_ready_observed'] = False
        with self.assertRaises(ValueError):
            freezer.validate_evidence(evidence)

    def test_binds_source_and_compiler_dependency_hashes_exactly(self):
        check = freezer.validate_hash_bindings
        check({'rust/src/a.rs': 'a', 'compiler/lib.rlib': 'b'},
              {'rust/src/a.rs': 'a', 'compiler/lib.rlib': 'b'})
        for name, actual in [
            ('stale source', {'rust/src/a.rs': 'changed', 'compiler/lib.rlib': 'b'}),
            ('changed dependency', {'rust/src/a.rs': 'a', 'compiler/lib.rlib': 'changed'}),
            ('missing dependency', {'rust/src/a.rs': 'a'}),
        ]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                check({'rust/src/a.rs': 'a', 'compiler/lib.rlib': 'b'}, actual)


if __name__ == '__main__':
    unittest.main()
