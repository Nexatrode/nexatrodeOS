import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('owner_validation', ROOT / 'platforms/sunfish/validate-modem-owner.py')
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)
ufs_pool_spec = importlib.util.spec_from_file_location(
    'ufs_pool_validation', ROOT / 'platforms/sunfish/validate-ufs-dma-pool.py'
)
ufs_pool_validation = importlib.util.module_from_spec(ufs_pool_spec)
ufs_pool_spec.loader.exec_module(ufs_pool_validation)
wifi_trace_spec = importlib.util.spec_from_file_location(
    'wifi_trace_runner', ROOT / 'platforms/sunfish/run-msm-bus-wifi-comparison.py'
)
wifi_trace_runner = importlib.util.module_from_spec(wifi_trace_spec)
wifi_trace_spec.loader.exec_module(wifi_trace_runner)


class OwnerManifestTests(unittest.TestCase):
    def setUp(self):
        self.system = ET.parse(ROOT / 'platforms/sunfish/modem-owner.system').getroot()

    def test_exact_profile(self):
        validator.validate(self.system)

    def test_wrong_rights_cache_address_and_size(self):
        for name, key, value in [('smem_snapshot_high', 'perms', 'rw'), ('smem_ap_page', 'cached', 'true'),
                                 ('glink_prepare_tail', 'vaddr', '0x50000000')]:
            bad = copy.deepcopy(self.system)
            bad.find(f"protection_domain/map[@mr='{name}']").set(key, value)
            with self.assertRaises(AssertionError):
                validator.validate(bad)
        self.system.find("memory_region[@name='smem_ap_page']").set('size', '0x2000')
        with self.assertRaises(AssertionError):
            validator.validate(self.system)

    def test_physical_alias_rejected(self):
        ET.SubElement(self.system, 'memory_region', name='alias', size='0x1000', phys_addr='0x8611d000')
        ET.SubElement(self.system.find('protection_domain'), 'map', mr='alias', vaddr='0x70000000', perms='rw', cached='false')
        with self.assertRaises(AssertionError):
            validator.validate(self.system)

    def test_irq_route_trigger_and_extra_cap_rejected(self):
        for key, value in [('irq', '449'), ('id', '2'), ('trigger', 'level')]:
            bad = copy.deepcopy(self.system)
            bad.find('protection_domain/irq').set(key, value)
            with self.assertRaises(AssertionError):
                validator.validate(bad)

    def test_missing_continuation_rejected(self):
        self.system.remove(self.system.find('channel'))
        with self.assertRaises(AssertionError):
            validator.validate(self.system)

    def test_shared_owner_domain_rejected(self):
        ET.SubElement(self.system, 'protection_domain', name='other')
        with self.assertRaises(AssertionError):
            validator.validate(self.system)

    def test_insufficient_stack_rejected(self):
        self.system.find('protection_domain').set('stack_size', '0x20000')
        with self.assertRaises(AssertionError):
            validator.validate(self.system)


class UfsCandidateManifestTests(unittest.TestCase):
    def setUp(self):
        self.system = ET.parse(ROOT / 'platforms/sunfish/modem-owner-ufs-profile.system').getroot()

    def test_candidate_has_build_allocated_backing_pool(self):
        validator.validate(self.system, include_ufs=True)

    def test_physical_address_and_mapping_contract_are_pinned(self):
        for mutate in [
            lambda root: root.find("memory_region[@name='ufs_dma_pool']").set('phys_addr', '0x88000000'),
            lambda root: root.find("protection_domain/map[@mr='ufs_dma_pool']").set('cached', 'true'),
            lambda root: root.find("protection_domain/setvar[@symbol='ufs_dma_pool_paddr']").set(
                'region_paddr', 'modem_heap'
            ),
            lambda root: root.find("memory_region[@name='ufs_smmu_tables']").set('size', '0xa000'),
            lambda root: root.find("protection_domain/map[@mr='ufs_smmu_tables']").set('cached', 'true'),
            lambda root: root.find("protection_domain/setvar[@symbol='ufs_smmu_tables_paddr']").set(
                'region_paddr', 'ufs_dma_pool'
            ),
        ]:
            candidate = copy.deepcopy(self.system)
            mutate(candidate)
            with self.assertRaises(AssertionError):
                validator.validate(candidate, include_ufs=True)


class OwnerCapdlTests(unittest.TestCase):
    def fixture(self):
        rights = {'write': True, 'read': False}
        objects = [{'name': 'cnode_modem_image_probe', 'object': {'CNode': {'slots': [
            {'slot': 1, 'cap': {'Notification': {'object': 9}}},
            {'slot': 12, 'cap': {'Notification': {'object': 9, 'badge': 8, 'rights': rights}}},
            {'slot': 138, 'cap': {'ArmIrqHandler': {'object': 1}}},
            {'slot': 139, 'cap': {'ArmIrqHandler': {'object': 2}}},
        ]}}}]
        for badge in [1, 2]:
            objects.append({'object': {'ArmIrq': {'slots': [{'cap': {'Notification': {'object': 9, 'badge': badge}}}], 'extra': {'trigger': 1}}}})
        objects.append({'name': 'pgd_modem_image_probe', 'object': {'PageTable': {'is_root': True, 'level': 0, 'slots': []}}})
        objects.append({'name': 'frame_mr_wlan_msa_pool_000000000', 'object': {'Frame': {'paddr': 0x98d00000, 'size_bits': 12}}})
        return {'objects': objects, 'irqs': [{'irq': 481, 'handler': 1}, {'irq': 483, 'handler': 2}]}

    def test_exact_kernel_capability_routing(self):
        validator.validate_capdl(self.fixture())

    def test_wrong_continuation_and_irq_badges_rejected(self):
        for mutate in [lambda d: d['objects'][0]['object']['CNode']['slots'][1]['cap']['Notification'].update(badge=4),
                       lambda d: d['objects'][1]['object']['ArmIrq']['slots'][0]['cap']['Notification'].update(badge=2)]:
            spec = self.fixture()
            mutate(spec)
            with self.assertRaises(AssertionError):
                validator.validate_capdl(spec)

    def test_msa_frame_capability_and_mapping_rejected(self):
        for expose_in_cnode in [True, False]:
            spec = self.fixture()
            msa_frame = next(i for i, obj in enumerate(spec['objects']) if obj.get('name', '').startswith('frame_mr_wlan_msa_pool_'))
            if expose_in_cnode:
                spec['objects'][0]['object']['CNode']['slots'].append({'slot': 200, 'cap': {'Frame': {'object': msa_frame}}})
            else:
                root = next(obj for obj in spec['objects'] if obj.get('name') == 'pgd_modem_image_probe')
                root['object']['PageTable']['slots'].append({'slot': 0, 'cap': {'Frame': {'object': msa_frame}}})
            with self.assertRaises(AssertionError):
                validator.validate_capdl(spec)


class UfsPoolCapdlTests(unittest.TestCase):
    @staticmethod
    def fixture():
        root = {'name': 'pgd_modem_image_probe', 'object': {'PageTable': {
            'is_root': True, 'level': 0, 'slots': [
                {'slot': 0, 'cap': {'Frame': {'object': 1}}},
                {'slot': 1, 'cap': {'Frame': {'object': 2}}},
            ],
        }}}
        objects = [
            root,
            {'name': 'frame_mr_ufs_dma_pool_000000000', 'object': {'Frame': {
                'paddr': 0x88000000, 'size_bits': 12,
            }}},
            {'name': 'frame_mr_ufs_dma_pool_000000001', 'object': {'Frame': {
                'paddr': 0x88001000, 'size_bits': 12,
            }}},
        ]
        for index in range(11):
            object_id = len(objects)
            objects.append({'name': f'frame_mr_ufs_smmu_tables_{index:09d}', 'object': {'Frame': {
                'paddr': 0x89000000 + index * 0x1000, 'size_bits': 12,
            }}})
            root['object']['PageTable']['slots'].append({
                'slot': 2 + index, 'cap': {'Frame': {'object': object_id}},
            })
        return {'objects': objects}

    def test_contiguous_pages_mapped_once_in_owner(self):
        self.assertEqual(
            ufs_pool_validation.validate_capdl_pool(self.fixture(), expected_size=0x2000),
            (0x88000000, 0x2000),
        )

    def test_noncontiguous_or_aliased_pool_rejected(self):
        for mutate in [
            lambda spec: spec['objects'][2]['object']['Frame'].update(paddr=0x88002000),
            lambda spec: spec['objects'][0]['object']['PageTable']['slots'].append(
                {'slot': 2, 'cap': {'Frame': {'object': 1}}}
            ),
        ]:
            candidate = self.fixture()
            mutate(candidate)
            with self.assertRaises(ValueError):
                ufs_pool_validation.validate_capdl_pool(candidate, expected_size=0x2000)

    def test_smmu_tables_are_contiguous_separate_and_mapped_once(self):
        self.assertEqual(
            ufs_pool_validation.validate_smmu_table_frames(
                self.fixture(), pool_base=0x88000000, pool_size=0x2000,
            ),
            (0x89000000, 0xB000),
        )

    def test_table_pool_overlap_noncontiguity_and_aliases_rejected(self):
        for mutate in [
            lambda spec: [
                obj['object']['Frame'].__setitem__('paddr', 0x88000000 + i * 0x1000)
                for i, obj in enumerate(
                    item for item in spec['objects']
                    if item.get('name', '').startswith('frame_mr_ufs_smmu_tables_')
                )
            ],
            lambda spec: next(
                obj['object']['Frame'].__setitem__('paddr', 0x8900c000)
                for obj in spec['objects']
                if obj.get('name') == 'frame_mr_ufs_smmu_tables_000000010'
            ),
            lambda spec: spec['objects'][0]['object']['PageTable']['slots'].append(
                {'slot': 20, 'cap': {'Frame': {'object': 3}}}
            ),
        ]:
            candidate = self.fixture()
            mutate(candidate)
            with self.assertRaises(ValueError):
                ufs_pool_validation.validate_smmu_table_frames(
                    candidate, pool_base=0x88000000, pool_size=0x2000,
                )


class WifiTraceNormalizationTests(unittest.TestCase):
    def test_ansi_color_is_removed_before_timestamped_audit(self):
        self.assertEqual(
            wifi_trace_runner.strip_ansi("\x1b[32m[  210.703489] \x1b[0mupdate_request_adhoc: cl: 3"),
            "[  210.703489] update_request_adhoc: cl: 3",
        )

    def test_authorized_adb_serial_filter_ignores_emulator_when_targeted(self):
        output = (
            "List of devices attached\n"
            "emulator-5554 device product:sdk_gphone\n"
            "02281FQC200058 device product:sunfish\n"
            "offline-serial offline\n"
        )
        serials = wifi_trace_runner.authorized_adb_serials(output)
        self.assertEqual(serials, ["emulator-5554", "02281FQC200058"])
        self.assertIn("02281FQC200058", serials)

    def test_registration_log_filter_keeps_only_route_and_handle_records(self):
        text = (
            "[ 1.0] register_client_adhoc:find path.src 90 dest 512\n"
            "[ 1.1] wpa_supplicant: connected to private-network\n"
            "[ 1.2] register_client_adhoc:Client handle 10 ipa\n"
        )
        filtered = wifi_trace_runner.registration_lines(text)
        self.assertIn("find path.src 90 dest 512", filtered)
        self.assertIn("Client handle 10 ipa", filtered)
        self.assertNotIn("private-network", filtered)

    def test_live_registration_snapshot_requires_exact_ipa_path_set(self):
        snapshot = {
            "summary": {"registered_handles": 1},
            "registrations": [{
                "handle": 10,
                "name": "ipa",
                "paths": [
                    {"source_id": 90, "destination_id": 512, "route": ["ebi"]},
                    {"source_id": 90, "destination_id": 585, "route": ["imem"]},
                    {"source_id": 1, "destination_id": 676, "route": ["cfg"]},
                    {"source_id": 143, "destination_id": 777, "route": ["core"]},
                ],
            }],
        }
        self.assertEqual(
            wifi_trace_runner.validate_runtime_registration_snapshot(
                {"runtime_registration_snapshot": snapshot}
            ),
            snapshot["summary"],
        )
        snapshot["registrations"][0]["paths"].reverse()
        self.assertEqual(
            wifi_trace_runner.validate_runtime_registration_snapshot(
                {"runtime_registration_snapshot": snapshot}
            ),
            snapshot["summary"],
        )
        snapshot["registrations"][0]["paths"].reverse()
        snapshot["registrations"][0]["paths"].pop()
        with self.assertRaisesRegex(RuntimeError, "route set is incomplete"):
            wifi_trace_runner.validate_runtime_registration_snapshot(
                {"runtime_registration_snapshot": snapshot}
            )
        snapshot["registrations"][0]["paths"].append({
            "source_id": 90, "destination_id": 512, "route": ["ebi"],
        })
        snapshot["registrations"][0]["paths"][0].pop("route")
        with self.assertRaisesRegex(RuntimeError, "route set is incomplete"):
            wifi_trace_runner.validate_runtime_registration_snapshot(
                {"runtime_registration_snapshot": snapshot}
            )

    def test_runtime_join_marks_snapshot_path_mismatch_unresolved(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            trace = root / 'trace.txt'
            trace.write_text(
                '[ 1.010000] update_request_adhoc: cl: 10 index: 1 curr: 0 num_paths: 4\n'
                '[ 1.110000] update_request_adhoc: cl: 20 index: 1 curr: 0 num_paths: 2\n'
                '[ 1.120000] devbw soc:qcom,cpu-llcc-ddr-bw: BW MBps: AB: 1 IB: 2\n'
            )
            marker = root / 'markers.json'
            marker.write_text(json.dumps({
                'lineage': 'test',
                'events': [
                    {'event': 'wifi_enable_requested', 'uptime_seconds': 1.14},
                    {'event': 'wifi_on_connected', 'uptime_seconds': 1.2},
                ],
                'phases': {
                    'baseline': {'start_seconds': 1.0, 'end_seconds': 1.05},
                    'wifi_off': {'start_seconds': 1.05, 'end_seconds': 1.14},
                    'wifi_on_connected': {'start_seconds': 1.2, 'end_seconds': 1.3},
                },
            }))
            inventory = root / 'inventory.json'
            inventory.write_text(json.dumps({'runtime_registration_snapshot': {
                'scope': 'test snapshot',
                'registrations': [
                    {'handle': 10, 'name': 'ipa', 'paths': [{}, {}, {}, {}]},
                    {'handle': 20, 'name': 'grp3d', 'paths': [{}]},
                ],
            }}))
            report = wifi_trace_runner.comparison_report(trace, marker, inventory, root / 'report.json')
        self.assertEqual(report['summary']['path_count_mismatches'], 1)
        self.assertEqual(report['summary']['updates_with_path_count_match'], 1)
        self.assertIn('ipa#10', report['windows'][0]['updates_by_client'])
        self.assertEqual(
            report['windows'][0]['updates_by_client']['ipa#10']['usecase_transitions'],
            {'MIN->SVS2': 1},
        )
        self.assertEqual(
            report['windows'][0]['updates_by_client']['ipa#10']['events'][0]['to_ipa_rate_selector_hz'],
            50_000_000,
        )
        self.assertIn('snapshot-mismatch:grp3d#20', report['windows'][1]['updates_by_client'])

