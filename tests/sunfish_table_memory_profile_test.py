#!/usr/bin/env python3
"""Negative controls for the isolated eight-frame SMMU memory profile."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / 'platforms/sunfish/sunfish-table-memory-profile.py'
SPEC = importlib.util.spec_from_file_location('table_memory_profile', HELPER)
MODULE = importlib.util.module_from_spec(SPEC) if HELPER.exists() else None
if MODULE:
    SPEC.loader.exec_module(MODULE)

TABLES = ['smmu_root', 'smmu_l2', 'smmu_l3', 'smmu_upper',
          'smmu_l2_peer', 'smmu_l3_peer', 'smmu_l3_shared', 'smmu_l3_receive']


def fixture(experimental):
    root = ET.Element('system')
    pd = ET.SubElement(root, 'protection_domain', name='native_startup_preflight')
    ET.SubElement(root, 'protection_domain', name='other')
    objects = []
    for i, name in enumerate(TABLES):
        ET.SubElement(root, 'memory_region', name=name, size='0x1000')
        ET.SubElement(pd, 'map', mr=name, vaddr=hex(0x71001000 + i * 0x2000),
                      perms='rw', cached='false', **({'normal_nc': 'true'} if experimental else {}))
        objects.append(dict(name='frame_mr_' + name + '_000000000',
                            object={'Frame': dict(size_bits=12, paddr=0x80001000 + i * 0x1000,
                                                  init={'entries': []})}))
    objects += [dict(name='pgd_native_startup_preflight', object={'PageTable':
                    dict(level=0, is_root=True, x86_ept=False, slots=[dict(slot=0, cap={'PageTable': {'object': 9}})])}),
                dict(name='level1', object={'PageTable':
                    dict(level=1, is_root=False, x86_ept=False, slots=[dict(slot=1, cap={'PageTable': {'object': 10}})])}),
                dict(name='level2', object={'PageTable':
                    dict(level=2, is_root=False, x86_ept=False, slots=[dict(slot=392, cap={'PageTable': {'object': 11}})])}),
                dict(name='level3', object={'PageTable': dict(level=3, is_root=False, x86_ept=False, slots=[])}),
                dict(name='tcb_native_startup_preflight', object={'Tcb':
                    dict(slots=[dict(slot=1, cap={'PageTable': {'object': 8}})])})]
    for i in range(8):
        cap = dict(object=i, cached=False, executable=False,
                   rights=dict(read=True, write=True, grant=False, grant_reply=False))
        if experimental:
            cap['normal_nc'] = True
        objects[11]['object']['PageTable']['slots'].append(dict(slot=1 + i * 2, cap={'Frame': cap}))
    return root, dict(objects=objects)


class TableMemoryProfileTests(unittest.TestCase):
    def audit(self, root, model, experimental=False):
        self.assertIsNotNone(MODULE, 'memory-profile audit has not been implemented')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'profile.system'
            ET.ElementTree(root).write(path)
            return MODULE.audit(path, model, experimental=experimental)

    def reject(self, root, model, experimental=True):
        with self.assertRaises(ValueError):
            self.audit(root, model, experimental)

    def test_baseline_and_exact_experiment_are_accepted(self):
        for enabled in [False, True]:
            with self.subTest(enabled=enabled):
                result = self.audit(*fixture(enabled), experimental=enabled)
                self.assertEqual(result['table_frames'], 8)
                self.assertEqual(result['normal_nc_frames'], 8 if enabled else 0)

    def test_baseline_rejects_normal_nc_in_either_input(self):
        root, model = fixture(False)
        root.find('.//map').set('normal_nc', 'true')
        self.reject(root, model, False)
        root, model = fixture(False)
        model['objects'][11]['object']['PageTable']['slots'][0]['cap']['Frame']['normal_nc'] = True
        self.reject(root, model, False)

    def test_experiment_rejects_each_missing_table_flag(self):
        for i in range(8):
            for source in ['xml', 'capdl']:
                with self.subTest(table=i, source=source):
                    root, model = fixture(True)
                    if source == 'xml':
                        root.findall('.//map')[i].attrib.pop('normal_nc')
                    else:
                        model['objects'][11]['object']['PageTable']['slots'][i]['cap']['Frame'].pop('normal_nc')
                    self.reject(root, model)

    def test_xml_boolean_values_are_strict(self):
        for field in ['normal_nc', 'cached']:
            for value in ['1', '0', 'True', 'FALSE', '', 'yes']:
                with self.subTest(field=field, value=value):
                    root, model = fixture(True)
                    root.find('.//map').set(field, value)
                    self.reject(root, model)

    def test_capdl_boolean_values_are_strict(self):
        for field in ['normal_nc', 'cached', 'executable']:
            for value in [0, 1, 'false', None]:
                with self.subTest(field=field, value=value):
                    root, model = fixture(True)
                    model['objects'][11]['object']['PageTable']['slots'][0]['cap']['Frame'][field] = value
                    self.reject(root, model)
        for value in [0, 1, 'true', None]:
            root, model = fixture(True)
            model['objects'][11]['object']['PageTable']['slots'][0]['cap']['Frame']['rights']['read'] = value
            self.reject(root, model)

    def test_xml_table_geometry_rights_and_owner_are_pinned(self):
        for field, value in [('vaddr', '0x71002000'), ('perms', 'rwx'), ('perms', 'r'), ('cached', 'true')]:
            root, model = fixture(True)
            root.find('.//map').set(field, value)
            self.reject(root, model)
        for field, value in [('size', '0x2000'), ('page_size', '0x2000'), ('phys_addr', '0x80001000')]:
            root, model = fixture(True)
            root.find('memory_region').set(field, value)
            self.reject(root, model)
        root, model = fixture(True)
        pd, other = root.findall('protection_domain')
        mapping = pd.find('map'); pd.remove(mapping); other.append(mapping)
        self.reject(root, model)

    def test_xml_duplicate_regions_maps_pd_and_missing_region_are_rejected(self):
        for kind in ['region', 'map', 'pd', 'missing_region']:
            root, model = fixture(True)
            if kind == 'region': root.append(copy.deepcopy(root.find('memory_region')))
            if kind == 'map': root.find('protection_domain').append(copy.deepcopy(root.find('.//map')))
            if kind == 'pd': root.append(copy.deepcopy(root.find('protection_domain')))
            if kind == 'missing_region': root.remove(root.find('memory_region'))
            self.reject(root, model)

    def test_normal_nc_outside_the_eight_ram_maps_is_rejected(self):
        for physical in [False, True]:
            for enabled in [False, True]:
                root, model = fixture(enabled)
                attrs = dict(name='other_ram', size='0x1000')
                if physical: attrs['phys_addr'] = '0x15000000'
                ET.SubElement(root, 'memory_region', **attrs)
                ET.SubElement(root.findall('protection_domain')[1], 'map', mr='other_ram',
                              vaddr='0x70000000', cached='false', normal_nc='true')
                self.reject(root, model, enabled)
        root, model = fixture(True)
        model['objects'].append(dict(name='other_cap', object={'CNode': dict(slots=[dict(slot=1,
             cap={'Frame': dict(object=0, normal_nc=True, cached=False, executable=False,
                  rights=dict(read=True, write=True, grant=False, grant_reply=False))})])}))
        self.reject(root, model)

    def test_frame_init_size_identity_and_physical_aliases_are_rejected(self):
        for kind in ['fill', 'size', 'duplicate_name', 'duplicate_region_frame', 'physical_alias']:
            root, model = fixture(True)
            frame = model['objects'][0]['object']['Frame']
            if kind == 'fill': frame['init']['entries'] = [dict(range={'start': 0, 'end': 8})]
            if kind == 'size': frame['size_bits'] = 13
            if kind == 'duplicate_name': model['objects'].append(copy.deepcopy(model['objects'][0]))
            if kind == 'duplicate_region_frame':
                extra = copy.deepcopy(model['objects'][0]); extra['name'] = 'frame_mr_smmu_root_000000001'
                model['objects'].append(extra)
            if kind == 'physical_alias':
                extra = copy.deepcopy(model['objects'][0]); extra['name'] = 'frame_mr_other_000000000'
                model['objects'].append(extra)
            self.reject(root, model)

    def test_final_cap_rights_cache_exec_target_and_mapping_are_pinned(self):
        for kind in ['read', 'write', 'grant', 'grant_reply', 'cached', 'executable',
                     'target', 'slot', 'duplicate_slot', 'duplicate_cap', 'unreachable', 'wrong_tcb', 'wrong_level']:
            root, model = fixture(True)
            leaf = model['objects'][11]['object']['PageTable']; entry = leaf['slots'][0]
            cap = entry['cap']['Frame']
            if kind in ['read', 'write', 'grant', 'grant_reply']: cap['rights'][kind] = not cap['rights'][kind]
            if kind in ['cached', 'executable']: cap[kind] = True
            if kind == 'target': cap['object'] = 1
            if kind == 'slot': entry['slot'] = 2
            if kind == 'duplicate_slot': leaf['slots'].append(copy.deepcopy(entry))
            if kind == 'duplicate_cap': leaf['slots'].append(dict(slot=20, cap=copy.deepcopy(entry['cap'])))
            if kind == 'unreachable': model['objects'][10]['object']['PageTable']['slots'] = []
            if kind == 'wrong_tcb': model['objects'][12]['name'] = 'tcb_other'
            if kind == 'wrong_level': leaf['level'] = 2
            self.reject(root, model)

    def test_shared_subtable_and_cycle_do_not_hide_aliases(self):
        root, model = fixture(True)
        model['objects'][10]['object']['PageTable']['slots'].append(dict(slot=393, cap={'PageTable': {'object': 11}}))
        self.reject(root, model)
        root, model = fixture(True)
        model['objects'][10]['object']['PageTable']['slots'][0]['cap']['PageTable']['object'] = 10
        self.reject(root, model)

    def test_normal_nc_cannot_hide_on_non_mapping_xml_or_capdl_nodes(self):
        for enabled in [False, True]:
            for location in ['xml_root', 'xml_region', 'model_root', 'model_frame', 'cap_extra']:
                with self.subTest(enabled=enabled, location=location):
                    root, model = fixture(enabled)
                    if location == 'xml_root': root.set('normal_nc', 'true')
                    if location == 'xml_region': root.find('memory_region').set('normal_nc', 'true')
                    if location == 'model_root': model['normal_nc'] = True
                    if location == 'model_frame': model['objects'][0]['object']['Frame']['normal_nc'] = True
                    if location == 'cap_extra': model['objects'][11]['object']['PageTable']['slots'][0]['cap']['normal_nc'] = True
                    self.reject(root, model, enabled)
        for value in [1, 'false', None]:
            root, model = fixture(False); model['normal_nc'] = value
            self.reject(root, model, False)

    def test_table_region_must_be_a_top_level_ram_declaration(self):
        root, model = fixture(True)
        region = root.find('memory_region'); root.remove(region); root.find('protection_domain').append(region)
        self.reject(root, model)

    def test_other_xml_mapping_cannot_overlap_a_table_virtual_frame(self):
        root, model = fixture(True)
        ET.SubElement(root, 'memory_region', name='overlap', size='0x2000')
        ET.SubElement(root.find('protection_domain'), 'map', mr='overlap',
                      vaddr='0x71000000', cached='false', perms='rw')
        self.reject(root, model)

    def test_other_tcb_or_root_cannot_alias_the_native_table_mapping(self):
        for kind in ['tcb', 'root']:
            root, model = fixture(True)
            if kind == 'tcb':
                extra = copy.deepcopy(model['objects'][12]); extra['name'] = 'tcb_other'
            else:
                extra = copy.deepcopy(model['objects'][8]); extra['name'] = 'pgd_other'
            model['objects'].append(extra)
            self.reject(root, model)

    def test_reusable_negative_controls_reject_all_mutations(self):
        self.assertIsNotNone(MODULE)
        self.assertTrue(hasattr(MODULE, 'negative_controls'), 'reusable negative controls are missing')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'profile.system'
            for enabled in [False, True]:
                root, model = fixture(enabled)
                ET.ElementTree(root).write(path)
                labels = MODULE.negative_controls(path, model, experimental=enabled)
                self.assertGreaterEqual(len(labels), 24)
                self.assertEqual(len(labels), len(set(labels)))
                self.assertIn('other XML Normal-NC', labels)
                self.assertIn('table initialized', labels)
                self.assertIn('table alias', labels)


if __name__ == '__main__':
    unittest.main()
