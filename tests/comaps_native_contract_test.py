"""The offline map fixture must not acquire any app-data or network authority."""
import importlib.util,sys,unittest
from pathlib import Path
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'experiments/comaps-native'))
spec=importlib.util.spec_from_file_location('comaps_builder',ROOT/'experiments/comaps-native/build.py')
builder=importlib.util.module_from_spec(spec);spec.loader.exec_module(builder)
class Contract(unittest.TestCase):
    def setUp(self):
        self.system=ET.Element('system')
        self.maps={'slint_frame':'rw','map_heap':'rw','input_handoff':'rw','ui_focus':'rw','ui_pointer':'rw'}
        for n in self.maps:ET.SubElement(self.system,'memory_region',name=n,size='0x1000')
        for name in ('offline_map','gpu_client','gpu_driver','gpu_virt','input_driver','timer_driver','timer_poller'):
            d=ET.SubElement(self.system,'protection_domain',name=name)
            if name=='offline_map':
                for i,(mr,perms) in enumerate(self.maps.items()):ET.SubElement(d,'map',mr=mr,perms=perms,vaddr=hex(0x60000000+4096*i))
    def test_expected_diskless_contract(self):builder.validate(self.system)
    def test_app_cannot_add_wallet_memory(self):
        ET.SubElement(self.system,'memory_region',name='wallet_secret',size='0x1000')
        d=self.system.find("protection_domain[@name='offline_map']")
        ET.SubElement(d,'map',mr='wallet_secret',perms='r',vaddr='0x71000000')
        with self.assertRaises(AssertionError):builder.validate(self.system)
    def test_no_network_service(self):
        ET.SubElement(self.system,'protection_domain',name='https_worker')
        with self.assertRaises(AssertionError):builder.validate(self.system)
    def test_no_vault_region(self):
        ET.SubElement(self.system,'memory_region',name='vault',size='0x1000')
        with self.assertRaises(AssertionError):builder.validate(self.system)
    def test_no_overlapping_mappings(self):
        self.system.find("protection_domain[@name='offline_map']/map[@mr='map_heap']").set('vaddr','0x60000000')
        with self.assertRaises(AssertionError):builder.validate(self.system)
if __name__=='__main__':unittest.main()
