"""The standalone SMC owner has only splash memory and no client channels."""
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

class ScmPasQueryProbeTests(unittest.TestCase):
    def test_only_splash_and_one_isolated_smc_owner(self):
        root = ET.parse(ROOT / 'platforms/sunfish/scm-pas-query-probe.system').getroot()
        self.assertEqual([n.tag for n in root], ['memory_region', 'protection_domain'])
        region = root.find('memory_region')
        self.assertEqual(region.get('name'), 'splash')
        self.assertEqual(int(region.get('phys_addr'), 0), 0x9c000000)
        self.assertEqual(int(region.get('size'), 0), 0x1600000)
        pd = root.find('protection_domain')
        self.assertEqual(pd.get('smc'), 'true')
        self.assertEqual([n.tag for n in pd], ['program_image', 'map'])
        self.assertEqual(pd.find('program_image').get('path'), 'scm-pas-query-probe.elf')
        mapping = pd.find('map')
        self.assertEqual(mapping.get('mr'), 'splash')
        self.assertEqual(int(mapping.get('vaddr'), 0), 0x50000000)
        self.assertEqual(mapping.get('perms'), 'rw')
        self.assertEqual(mapping.get('cached'), 'false')

if __name__ == '__main__':
    unittest.main()
