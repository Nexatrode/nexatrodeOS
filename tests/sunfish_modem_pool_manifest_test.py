import copy
import importlib.util
from pathlib import Path
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('pool_validation', ROOT/'platforms/sunfish/validate-modem-pools.py')
v = importlib.util.module_from_spec(spec); spec.loader.exec_module(v)


def fixture():
    objects = []
    leaves = []
    for name, base, size, virtual, mapped in v.spans():
        for at in range(0, size, 4096):
            i = len(objects)
            objects.append({'name': f'frame_mr_{name}_{at//4096:09d}', 'object': {'Frame': {'size_bits': 12, 'paddr': base+at}}})
            if mapped:
                leaves.append((i, virtual+at))
    for at in range(0, v.WLAN_MSA[1], 4096):
        objects.append({'name': f'frame_mr_wlan_msa_pool_{at//4096:09d}',
                        'object': {'Frame': {'size_bits': 12, 'paddr': v.WLAN_MSA[0]+at}}})
    def table(level, root=False):
        i = len(objects)
        objects.append({'name': 'pgd_modem_image_probe' if root else f'table_{i}', 'object': {'PageTable': {'is_root': root, 'level': level, 'slots': []}}})
        return i
    root = table(0, True)
    for frame, virtual in leaves:
        parent = root
        for level in range(4):
            slot = (virtual >> (39-9*level)) & 511
            slots = objects[parent]['object']['PageTable']['slots']
            if level == 3:
                slots.append({'slot': slot, 'cap': {'Frame': {'object': frame, 'rights': {'read': True, 'write': True}, 'executable': False, 'cached': False}}})
            else:
                found = next((s for s in slots if s['slot'] == slot), None)
                if found is None:
                    child = table(level+1)
                    found = {'slot': slot, 'cap': {'PageTable': {'object': child}}}
                    slots.append(found)
                parent = found['cap']['PageTable']['object']
    return {'objects': objects}


def frame_slot(s):
    for o in s['objects']:
        for slot in o['object'].get('PageTable', {}).get('slots', []):
            if 'Frame' in slot['cap']:
                return o, slot
    raise AssertionError('no mapped frame')


class PoolManifestTests(unittest.TestCase):
    def setUp(self):
        self.tree = ET.parse(ROOT/'platforms/sunfish/modem-owner.system').getroot()

    def test_exact_reservations(self):
        v.validate_manifest(self.tree)

    def test_wlan_msa_must_match_cached_fixed_unmapped_profile(self):
        for attr, value in [('phys_addr', '0x98d01000'), ('size', '0x100000'), ('page_size', '0x200000')]:
            bad = copy.deepcopy(self.tree)
            bad.find("memory_region[@name='wlan_msa_pool']").set(attr, value)
            with self.assertRaises(AssertionError): v.validate_manifest(bad)
        bad = copy.deepcopy(self.tree)
        ET.SubElement(bad.find('protection_domain'), 'map', mr='wlan_msa_pool', vaddr='0x70000000', perms='rw', cached='false')
        with self.assertRaises(AssertionError): v.validate_manifest(bad)
        self.tree.remove(self.tree.find("memory_region[@name='wlan_msa_pool']"))
        with self.assertRaises(AssertionError): v.validate_manifest(self.tree)

    def test_physical_address_size_page_and_virtual_mutations(self):
        for selector, attr, value in [
            ("memory_region[@name='modem_storage']", 'phys_addr', '0xa2002000'),
            ("memory_region[@name='modem_heap']", 'size', '0x600000'),
            ("memory_region[@name='modem_heap']", 'page_size', '0x200000'),
            ("protection_domain/map[@mr='modem_scm_args']", 'vaddr', '0x64001000')]:
            bad = copy.deepcopy(self.tree); bad.find(selector).set(attr, value)
            with self.assertRaises(AssertionError): v.validate_manifest(bad)

    def test_pool_cache_execute_and_readonly_rejected(self):
        for attr, value in [('cached', 'true'), ('perms', 'rwx'), ('perms', 'r')]:
            bad = copy.deepcopy(self.tree); bad.find("protection_domain/map[@mr='modem_heap']").set(attr, value)
            with self.assertRaises(AssertionError): v.validate_manifest(bad)

    def test_physical_and_virtual_aliases_rejected(self):
        for physical, virtual in [(0xa2001000, 0x70000000), (0xa3000000, 0x61001000)]:
            bad = copy.deepcopy(self.tree)
            ET.SubElement(bad, 'memory_region', name='alias', phys_addr=hex(physical), size='0x1000', page_size='0x1000')
            ET.SubElement(bad.find('protection_domain'), 'map', mr='alias', vaddr=hex(virtual), perms='rw', cached='false')
            with self.assertRaises(AssertionError): v.validate_manifest(bad)

    def test_any_guard_mapping_or_missing_reservation_rejected(self):
        for name, _, _, virtual, mapped in v.spans():
            if mapped: continue
            bad = copy.deepcopy(self.tree)
            ET.SubElement(bad.find('protection_domain'), 'map', mr=name, vaddr=hex(virtual), perms='r', cached='false')
            with self.assertRaises(AssertionError): v.validate_manifest(bad)
        self.tree.remove(self.tree.find("memory_region[@name='modem_heap_guard_low']"))
        with self.assertRaises(AssertionError): v.validate_manifest(self.tree)

    def test_physical_symbol_rebinding_rejected(self):
        self.tree.find("protection_domain/setvar[@symbol='modem_heap_paddr']").set('region_paddr', 'modem_storage')
        with self.assertRaises(AssertionError): v.validate_manifest(self.tree)

    def test_fixed_reservations_exclude_payloads_and_guards(self):
        for _, base, size, _, _ in v.spans():
            with self.assertRaises(AssertionError): v.validate_exclusions([('reserved', base, size, True)])
        for base, size in [(0, 0), (-1, 4096), (2**64-1, 2)]:
            with self.assertRaises(AssertionError): v.validate_exclusions([('invalid', base, size, False)])
        v.validate_exclusions([('SMEM', 0x86000000, 0x200000, True), ('WLAN MSA', 0x98d00000, 0x200000, True)], include_msa=False)
        with self.assertRaises(AssertionError):
            v.validate_exclusions([('candidate', v.WLAN_MSA[0], 4096, False)])


class PoolCapdlTests(unittest.TestCase):
    def test_actual_page_table_walk_and_all_guards(self):
        result = v.validate_capdl(fixture())
        self.assertEqual(result['reserved_pages'], 1799)
        self.assertEqual(result['unmapped_guard_pages'], 6)
        self.assertEqual(result['wlan_msa_reserved_pages'], 512)
        self.assertTrue(result['wlan_msa_unmapped'])

    def test_wlan_msa_capability_and_mapping_rejected(self):
        for cnode, root in [(True, False), (False, True)]:
            s = fixture()
            msa_frame = next(i for i, obj in enumerate(s['objects']) if obj['name'].startswith('frame_mr_wlan_msa_pool_'))
            if cnode:
                s['objects'].append({'name': 'cnode_modem_image_probe', 'object': {'CNode': {'slots': [
                    {'slot': 99, 'cap': {'Frame': {'object': msa_frame}}}
                ]}}})
            if root:
                pgd = next(obj for obj in s['objects'] if obj['name'] == 'pgd_modem_image_probe')
                pgd['object']['PageTable']['slots'].append({'slot': 1, 'cap': {'Frame': {'object': msa_frame}}})
            with self.assertRaises(AssertionError): v.validate_capdl(s)

    def test_frame_missing_larger_wrong_address_and_alias_rejected(self):
        for field, value in [('paddr', 0xa3000000), ('size_bits', 21)]:
            s = fixture();s['objects'][1]['object']['Frame'][field] = value
            with self.assertRaises(AssertionError): v.validate_capdl(s)
        s = fixture();s['objects'].append(copy.deepcopy(s['objects'][1]))
        with self.assertRaises(AssertionError): v.validate_capdl(s)

    def test_guard_capability_or_alias_cap_rejected(self):
        for object_id in [0, 1]:
            s = fixture()
            s['objects'].append({'name': 'extra', 'object': {'CNode': {'slots': [{'slot': 0, 'cap': {'Frame': {'object': object_id}}}]}}})
            with self.assertRaises(AssertionError): v.validate_capdl(s)

    def test_leaf_rights_cache_execute_rejected(self):
        for field, value in [('cached', True), ('executable', True), ('rights', {'read': True, 'write': False})]:
            s = fixture(); _, slot = frame_slot(s);slot['cap']['Frame'][field] = value
            with self.assertRaises(AssertionError): v.validate_capdl(s)

    def test_renamed_tables_do_not_fake_actual_virtual_address(self):
        s = fixture();obj, slot = frame_slot(s)
        # Rename is harmless: validator follows the actual tree from the root.
        obj['name'] = 'misleading_vaddr_0x0';v.validate_capdl(s)
        slot['slot'] += 400
        with self.assertRaises(AssertionError): v.validate_capdl(s)

    def test_unreachable_frame_duplicate_slot_and_cycle_rejected(self):
        for case in range(3):
            s = fixture();obj, slot = frame_slot(s)
            if case == 0: obj['object']['PageTable']['slots'].remove(slot)
            elif case == 1: obj['object']['PageTable']['slots'].append(copy.deepcopy(slot))
            else:
                root = next((i, o) for i, o in enumerate(s['objects']) if o['name'] == 'pgd_modem_image_probe')
                root[1]['object']['PageTable']['slots'][0]['cap']['PageTable']['object'] = root[0]
            with self.assertRaises(AssertionError): v.validate_capdl(s)

    def test_allocation_report_rejects_missing_dynamic_or_aliased_frame(self):
        s = fixture()
        def report():
            return '\n'.join(f"{i} - Page: '{o['name']}' @ {o['object']['Frame']['paddr']:#x}" for i, o in enumerate(s['objects']) if 'Frame' in o['object'])
        v.validate_report(s, report())
        before = report()
        s['objects'].append({'name': 'dynamic', 'object': {'Frame': {'paddr': None, 'size_bits': 12}}})
        with self.assertRaises(AssertionError): v.validate_report(s, before)
        aliased = before+f"\n{len(s['objects'])-1} - Page: 'dynamic' @ 0xa2001000"
        with self.assertRaises(AssertionError): v.validate_report(s, aliased)
        with self.assertRaises(AssertionError): v.validate_report(s, before+f"\n{len(s['objects'])-1} - Page: 'wrong' @ 0xa3000000")


if __name__ == '__main__':
    unittest.main()
