from pathlib import Path
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'experiments/desktop-apps'))
from runtime_manifest import APPS, inventory


class InventoryTest(unittest.TestCase):
    def setUp(self):
        self.system = ET.Element('system')
        for app, spec in APPS.items():
            ET.SubElement(self.system, 'memory_region', name=app, size='0x1000')
            for name in (spec['frontend'], *spec['services']):
                pd = ET.SubElement(self.system, 'protection_domain', name=name)
                ET.SubElement(pd, 'map', mr=app, perms='rw')
        ET.SubElement(self.system, 'protection_domain', name='vault')

    def result(self,maps=False):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'system.xml'
            ET.ElementTree(self.system).write(p)
            return inventory(p,maps=maps)

    def test_complete_groups_do_not_claim_lock_or_teardown(self):
        r = self.result()
        self.assertEqual(len(r['apps']['simplex']['services']), 5)
        self.assertEqual(len(r['apps']['nexawal']['services']), 5)
        self.assertFalse(r['guarantees']['wrapped_key_lock'])
        self.assertFalse(r['guarantees']['private_memory_cleanup_on_close'])

    def test_unknown_or_missing_worker_is_not_omitted(self):
        worker = self.system.find("protection_domain[@name='photo_worker']")
        worker.set('name', 'new_worker')
        with self.assertRaisesRegex(ValueError, 'unclassified lifecycle'):
            self.result()

    def test_cross_app_private_memory_is_rejected(self):
        pd = self.system.find("protection_domain[@name='native_browser']")
        ET.SubElement(pd, 'map', mr='nexawal', perms='r')
        with self.assertRaisesRegex(ValueError, 'private cross-app'):
            self.result()

    def test_maps_is_an_explicit_separate_app_group(self):
        ET.SubElement(self.system,'memory_region',name='map_heap',size='0x1000000')
        d=ET.SubElement(self.system,'protection_domain',name='offline_map')
        ET.SubElement(d,'map',mr='map_heap',perms='rw')
        with self.assertRaisesRegex(ValueError,'unclassified lifecycle'):self.result()
        result=self.result(maps=True)
        self.assertEqual(result['apps']['maps']['services'],[])
        ET.SubElement(d,'map',mr='nexawal',perms='r')
        with self.assertRaisesRegex(ValueError,'private cross-app'):self.result(maps=True)

    def test_maps_cannot_claim_the_curated_lock_guarantees(self):
        ET.SubElement(self.system,'protection_domain',name='offline_map')
        ET.SubElement(self.system,'protection_domain',name='lock_auth')
        with self.assertRaisesRegex(ValueError,'outside the curated'):self.result(maps=True)

    def test_rtc_readout_is_not_owned_private_memory(self):
        ET.SubElement(self.system,'memory_region',name='rtc',size='0x1000',phys_addr='0x9010000')
        pd=self.system.find("protection_domain[@name='browser_https']")
        m=ET.SubElement(pd,'map',mr='rtc',perms='r')
        result=self.result()['apps']['netsurf']
        self.assertIn('rtc',result['public_readonly_regions'])
        self.assertNotIn('rtc',result['private_regions'])
        m.set('perms','rw')
        with self.assertRaisesRegex(ValueError,'writable public'):self.result()

    def test_shared_clock_is_read_only_for_apps(self):
        ET.SubElement(self.system, 'memory_region', name='clock_view', size='0x1000')
        a = self.system.find("protection_domain[@name='chat_ui']")
        b = self.system.find("protection_domain[@name='wallet_ui']")
        ET.SubElement(a, 'map', mr='clock_view', perms='r')
        m = ET.SubElement(b, 'map', mr='clock_view', perms='r')
        self.result()
        m.set('perms', 'rw')
        with self.assertRaisesRegex(ValueError, 'writable public'):
            self.result()


if __name__ == '__main__':
    unittest.main()
