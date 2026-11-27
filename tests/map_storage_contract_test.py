"""The new disk authority is confined to the two Maps protection domains."""
import importlib.util,unittest
from pathlib import Path
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('map_storage_adapter',ROOT/'experiments/desktop-apps/maps.py')
maps=importlib.util.module_from_spec(spec);spec.loader.exec_module(maps)
class StorageAuthority(unittest.TestCase):
    def setUp(self):
        self.s=ET.Element('system')
        for n in ('slint_ui','vault','wallet_app','simplex_agent','native_browser','browser_https'):ET.SubElement(self.s,'protection_domain',name=n)
        def mr(name,size,**kw):ET.SubElement(self.s,'memory_region',name=name,size=hex(size),**kw)
        def mapping(pd,name,va,perms,symbol):ET.SubElement(pd,'map',mr=name,vaddr=hex(va),perms=perms,setvar_vaddr=symbol)
        def channel(a,i,b,j):
            c=ET.SubElement(self.s,'channel');ET.SubElement(c,'end',pd=a,id=str(i));ET.SubElement(c,'end',pd=b,id=str(j))
        maps.attach_mobile(self.s,self.pd('slint_ui'),mapping,mr,channel,storage=True)
    def pd(self,name):return self.s.find(f"protection_domain[@name='{name}']")
    def test_private_disk_and_readonly_payload(self):maps.validate_mobile(self.s)
    def test_renderer_cannot_write_region_bytes(self):
        self.pd('offline_map').find("map[@mr='map_store_bytes']").set('perms','rw')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_storage_cannot_access_vault(self):
        ET.SubElement(self.pd('map_store'),'map',mr='wallet_transfer',perms='r')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_shell_cannot_read_public_disk_buffer_without_grant(self):
        ET.SubElement(self.pd('slint_ui'),'map',mr='map_store_bytes',perms='r')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_no_extra_storage_endpoint(self):
        c=ET.SubElement(self.s,'channel');ET.SubElement(c,'end',pd='map_store',id='4');ET.SubElement(c,'end',pd='vault',id='31')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_no_access_to_other_block_queue(self):
        ET.SubElement(self.pd('map_store'),'map',mr='blk_client_vault_request',perms='rw')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
if __name__=='__main__':unittest.main()
