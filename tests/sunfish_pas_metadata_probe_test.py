"""Only dedicated contiguous metadata RAM and splash, without SMC or I/O."""
import unittest
import importlib.util
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

class PasMetadataProbeTests(unittest.TestCase):
    def test_owned_metadata_memory_is_nx_uncached_and_physically_identified(self):
        root = ET.parse(ROOT / 'platforms/sunfish/pas-metadata-probe.system').getroot()
        self.assertEqual([n.tag for n in root], ['memory_region'] * 2 + ['protection_domain'])
        regions = {n.get('name'): n for n in root.findall('memory_region')}
        self.assertEqual(set(regions), {'pas_metadata', 'splash'})
        metadata = regions['pas_metadata']
        self.assertIsNone(metadata.get('phys_addr'))
        self.assertEqual(int(metadata.get('size'), 0), 0x5000)
        self.assertEqual(int(metadata.get('page_size'), 0), 0x1000)
        self.assertEqual(int(regions['splash'].get('phys_addr'), 0), 0x9c000000)
        self.assertEqual(int(regions['splash'].get('size'), 0), 0x1600000)
        pd = root.find('protection_domain')
        self.assertNotEqual(pd.get('smc'), 'true')
        self.assertEqual([n.tag for n in pd], ['program_image', 'map', 'setvar', 'map'])
        self.assertEqual(pd.find('setvar').attrib, {'symbol':'pas_metadata_paddr', 'region_paddr':'pas_metadata'})
        maps = {n.get('mr'): n for n in pd.findall('map')}
        for name, vaddr in [('pas_metadata', 0x40000000), ('splash', 0x42000000)]:
            self.assertEqual(int(maps[name].get('vaddr'), 0), vaddr)
            self.assertEqual(maps[name].get('perms'), 'rw')
            self.assertEqual(maps[name].get('cached'), 'false')

    def test_layout_audit_refuses_missing_split_or_overlapping_physical_pages(self):
        spec = importlib.util.spec_from_file_location('pas_layout', ROOT / 'platforms/sunfish/validate-pas-metadata-layout.py')
        layout = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(layout)
        pages = [(0x80000000+i*0x1000, 0x1000, 'frame_mr_pas_metadata_'+str(i)) for i in range(5)]
        self.assertEqual(layout.validate(pages, [])['metadata_physical'], '0x80001000')
        for bad in [pages[:-1], pages + [pages[-1]], pages[:4] + [(0x80006000, 0x1000, pages[-1][2])]]:
            with self.assertRaises(ValueError):
                layout.validate(bad, [])
        with self.assertRaises(ValueError):
            layout.validate(pages, [('reserved', 0x80004000, 0x1000, True)])
        with self.assertRaises(ValueError):
            layout.validate(pages + [(0x80002000, 0x1000, 'other_frame')], [])

if __name__ == '__main__':
    unittest.main()
