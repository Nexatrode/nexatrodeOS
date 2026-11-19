"""The integrated preflight has bounded I/O authority, no secure/reset/DMA path."""
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class ModemPrepareProbeTests(unittest.TestCase):
    def test_only_metadata_splash_aop_and_apps_rsc_are_accessible(self):
        root = ET.parse(ROOT / 'platforms/sunfish/modem-prepare-probe.system').getroot()
        self.assertEqual([n.tag for n in root], ['memory_region'] * 6 + ['protection_domain'])
        regions = {n.get('name'): n for n in root.findall('memory_region')}
        pd = root.find('protection_domain')
        self.assertEqual(pd.attrib, {'name': 'modem_prepare_probe', 'priority': '99', 'stack_size': '0x20000'})
        self.assertEqual(pd.find('program_image').get('path'), 'modem_prepare_probe.elf')
        self.assertEqual([n.tag for n in pd], ['program_image'] + ['map'] * 5 + ['setvar', 'map'])
        self.assertEqual(pd.find('setvar').attrib, {'symbol': 'pas_metadata_paddr', 'region_paddr': 'pas_metadata'})
        maps = {n.get('mr'): n for n in pd.findall('map')}
        expected = {
            'apps_rsc': (0x18220000, 0x1000, 0x1000, 0x40000000, 'rw', 'apps_rsc'),
            'apps_rsc_tail': (0x18221000, 0x2000, 0x1000, 0x40001000, 'r', None),
            'aop_msgram': (0x0c300000, 0x1000, 0x1000, 0x40004000, 'rw', 'aop_msgram'),
            'aop_doorbell': (0x17c00000, 0x1000, 0x1000, 0x40005000, 'rw', 'aop_doorbell'),
            'pas_metadata': (None, 0x5000, 0x1000, 0x40010000, 'rw', 'pas_metadata'),
            'splash': (0x9c000000, 0x1600000, 0x200000, 0x42000000, 'rw', 'splash_fb'),
        }
        self.assertEqual(set(regions), set(expected))
        self.assertEqual(set(maps), set(expected))
        for name, (physical, size, page_size, virtual, perms, symbol) in expected.items():
            r, m = regions[name], maps[name]
            self.assertEqual(None if r.get('phys_addr') is None else int(r.get('phys_addr'), 0), physical)
            self.assertEqual(int(r.get('size'), 0), size)
            self.assertEqual(int(r.get('page_size'), 0), page_size)
            self.assertEqual(int(m.get('vaddr'), 0), virtual)
            self.assertEqual(m.get('perms'), perms)
            self.assertEqual(m.get('cached'), 'false')
            self.assertEqual(m.get('setvar_vaddr'), symbol)
            if physical is not None:
                self.assertFalse(physical < 0x94800000 and 0x8b000000 < physical + size)


if __name__ == '__main__':
    unittest.main()
