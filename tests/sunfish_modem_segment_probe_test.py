"""Keep the two-page data/BSS experiment isolated and its guards read-only."""
import copy
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from tests.sunfish_modem_data_probe_test import PORT, load


class ModemSegmentTests(unittest.TestCase):
    def test_exception_requires_the_exact_manifest_and_read_only_guards(self):
        audit = load('audit-capdl-memory')
        self.assertEqual(audit.fixed_modem_segment_pages(None), set())
        manifest = PORT / 'modem-segment-probe.system'
        self.assertEqual(len(audit.fixed_modem_segment_pages(manifest)), 4)
        original = ET.parse(manifest).getroot()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'changed.system'
            changes = []
            for name in ['modem_guard_low', 'modem_segment_data', 'modem_guard_high']:
                for attr, value in [('phys_addr', '0x93200000'), ('size', '0x4000'), ('page_size', '0x200000')]:
                    changes.append(("memory_region[@name='%s']" % name, attr, value))
                for attr, value in [('cached', 'true'), ('vaddr', '0x50000000'), ('perms', 'rwx')]:
                    changes.append((".//map[@mr='%s']" % name, attr, value))
            for name in ['modem_guard_low', 'modem_guard_high']:
                changes.append((".//map[@mr='%s']" % name, 'perms', 'rw'))
            for selector, attr, value in changes:
                changed = copy.deepcopy(original)
                changed.find(selector).set(attr, value)
                ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError): audit.fixed_modem_segment_pages(path)
            for node in [ET.Element('protection_domain', name='client'),
                         ET.Element('channel'),
                         copy.deepcopy(original.find("memory_region[@name='modem_segment_data']"))]:
                changed = copy.deepcopy(original)
                changed.append(node)
                ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError): audit.fixed_modem_segment_pages(path)
            changed = copy.deepcopy(original)
            pd = changed.find('protection_domain')
            pd.append(copy.deepcopy(pd.find("map[@mr='modem_segment_data']")))
            ET.ElementTree(changed).write(path)
            with self.assertRaises(ValueError): audit.fixed_modem_segment_pages(path)

    def test_only_four_exact_frames_are_admitted_and_normal_exclusion_remains(self):
        layout = load('validate-modem-segment-layout')
        old = load('validate-pas-mem-setup-layout')
        pool = [('modem_region@8b000000', 0x8b000000, 0x9800000, True)]
        other = [(0x80000000, 4096, 'metadata')]
        fixed = [(0x8b77f000, 4096, 'frame_mr_modem_guard_low_0'),
                 (0x8b780000, 4096, 'frame_mr_modem_segment_data_0'),
                 (0x8b781000, 4096, 'frame_mr_modem_segment_data_1'),
                 (0x8b782000, 4096, 'frame_mr_modem_guard_high_0')]
        def validate(pages):
            return layout.validate(pages, pool, pool, [4], [1], [1], PORT / 'modem-segment-probe.system')
        self.assertEqual(validate(other + fixed)['ap_frames_in_modem_reservation'], 4)
        with self.assertRaises(ValueError): old.validate(other + fixed, pool, pool, [4], [1], [1])
        for i in range(4):
            for replacement in [[], [fixed[i]], [(fixed[i][0]+4096, 4096, fixed[i][2])],
                                [(fixed[i][0], 8192, fixed[i][2])], [(fixed[i][0], 4096, 'alias')]]:
                pages = fixed[:i] + fixed[i+1:] + replacement
                if pages == fixed or sorted(pages) == sorted(fixed): continue
                with self.assertRaises(ValueError): validate(other + pages)
        for alias in [(0x8b780000,4096,'alias'), (0x93200000,4096,'other'), fixed[0]]:
            with self.assertRaises(ValueError): validate(other + fixed + [alias])

if __name__ == '__main__': unittest.main()
