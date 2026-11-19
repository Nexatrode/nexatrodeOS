"""Bound the experimental reserved-memory exception; preserve normal exclusion."""
import copy
import importlib.util
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PORT = ROOT / 'platforms/sunfish'

def load(name):
    spec = importlib.util.spec_from_file_location(name, PORT / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

class ModemDataTests(unittest.TestCase):
    def test_exception_is_opt_in_and_rejects_any_changed_authority(self):
        audit = load('audit-capdl-memory')
        self.assertEqual(audit.fixed_modem_data_pages(None), set())
        manifest = PORT / 'modem-data-probe.system'
        self.assertEqual(audit.fixed_modem_data_pages(manifest), {(0x93200000, 4096)})
        original = ET.parse(manifest).getroot()
        mutations = [
            ("memory_region[@name='modem_data_page']", 'phys_addr', '0x93201000'),
            ("memory_region[@name='modem_data_page']", 'size', '0x2000'),
            ("memory_region[@name='modem_data_page']", 'page_size', '0x200000'),
            (".//map[@mr='modem_data_page']", 'perms', 'rwx'),
            (".//map[@mr='modem_data_page']", 'cached', 'true'),
            (".//map[@mr='modem_data_page']", 'vaddr', '0x50000000'),
            ("protection_domain", 'smc', 'false'),
        ]
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / 'changed.system'
            for selector, attr, value in mutations:
                changed = copy.deepcopy(original)
                changed.find(selector).set(attr, value)
                ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError): audit.fixed_modem_data_pages(path)
            for extra in [ET.Element('protection_domain', name='client'),
                          copy.deepcopy(original.find("memory_region[@name='modem_data_page']"))]:
                changed = copy.deepcopy(original)
                changed.append(extra)
                ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError): audit.fixed_modem_data_pages(path)
            changed = copy.deepcopy(original)
            pd = changed.find('protection_domain')
            pd.append(copy.deepcopy(pd.find("map[@mr='modem_data_page']")))
            ET.ElementTree(changed).write(path)
            with self.assertRaises(ValueError): audit.fixed_modem_data_pages(path)

    def test_all_other_modem_frames_and_aliases_still_fail(self):
        layout = load('validate-modem-data-layout')
        old = load('validate-pas-mem-setup-layout')
        pool = [('modem_region@8b000000', 0x8b000000, 0x9800000, True)]
        other = [(0x80000000, 4096, 'metadata')]
        fixed = (0x93200000, 4096, 'frame_mr_modem_data_page_0')
        def validate(pages):
            return layout.validate(pages, pool, pool, [4], [1], [1], PORT / 'modem-data-probe.system')
        self.assertEqual(validate(other + [fixed])['ap_frames_in_modem_reservation'], 1)
        with self.assertRaises(ValueError):
            old.validate(other + [fixed], pool, pool, [4], [1], [1])
        for pages in [other, other + [fixed, fixed],
                      other + [(0x93200000, 8192, fixed[2])],
                      other + [(0x93201000, 4096, fixed[2])],
                      other + [(fixed[0], fixed[1], 'alias')],
                      other + [fixed, (0x93200000, 4096, 'alias')],
                      other + [fixed, (0x8b000000, 4096, 'other_modem_frame')]]:
            with self.assertRaises(ValueError): validate(pages)

if __name__ == '__main__': unittest.main()
