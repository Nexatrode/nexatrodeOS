"""Only the transaction first page is writable; sleep/wake pages stay read-only."""
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

class BcmReadsIsolationTests(unittest.TestCase):
    def test_fixed_controller_permissions_and_contiguous_snapshot_window(self):
        for manifest in ['bcm-reads-probe.system', 'bcm-mc0-hold-probe.system',
                         'rpmh-proxy-hold-probe.system', 'rpmh-pas-bus-probe.system']:
            with self.subTest(manifest=manifest):
                self.check_manifest(ROOT / 'platforms/sunfish' / manifest)

    def check_manifest(self, manifest):
        root = ET.parse(manifest).getroot()
        self.assertEqual([n.tag for n in root],['memory_region']*3+['protection_domain'])
        expected={
            'apps_rsc':(0x18220000,0x1000,0x40000000,'rw'),
            'apps_rsc_tail':(0x18221000,0x2000,0x40001000,'r'),
            'splash':(0x9c000000,0x1600000,0x42000000,'rw'),
        }
        regions={n.get('name'):n for n in root.findall('memory_region')}
        pd=root.find('protection_domain')
        self.assertNotEqual(pd.get('smc'),'true')
        self.assertEqual([n.tag for n in pd],['program_image']+['map']*3)
        maps={n.get('mr'):n for n in pd.findall('map')}
        self.assertEqual(set(regions),set(expected));self.assertEqual(set(maps),set(expected))
        for name,(physical,size,virtual,perms) in expected.items():
            self.assertEqual(int(regions[name].get('phys_addr'),0),physical)
            self.assertEqual(int(regions[name].get('size'),0),size)
            self.assertEqual(int(maps[name].get('vaddr'),0),virtual)
            self.assertEqual(maps[name].get('perms'),perms)
            self.assertEqual(maps[name].get('cached'),'false')

if __name__=='__main__':unittest.main()
