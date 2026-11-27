"""The phone map worker has no vault, networking or global input authority."""
import importlib.util,unittest
from pathlib import Path
import xml.etree.ElementTree as ET
spec=importlib.util.spec_from_file_location('mobile_maps_adapter',Path(__file__).resolve().parents[1]/'experiments/desktop-apps/maps.py')
maps=importlib.util.module_from_spec(spec);spec.loader.exec_module(maps)
class Contracts(unittest.TestCase):
    def setUp(self):
        self.s=ET.Element('system');self.ui=ET.SubElement(self.s,'protection_domain',name='slint_ui')
        for name in ('vault','wallet_app','simplex_agent','native_browser','browser_https'):ET.SubElement(self.s,'protection_domain',name=name)
        def mr(name,size,**kw):ET.SubElement(self.s,'memory_region',name=name,size=hex(size),**kw)
        def mapping(pd,name,va,perms,symbol):ET.SubElement(pd,'map',mr=name,vaddr=hex(va),perms=perms,setvar_vaddr=symbol)
        def channel(a,i,b,j):
            c=ET.SubElement(self.s,'channel');ET.SubElement(c,'end',pd=a,id=str(i));ET.SubElement(c,'end',pd=b,id=str(j))
        maps.attach_mobile(self.s,self.ui,mapping,mr,channel)
    def domain(self,name):return self.s.find(f"protection_domain[@name='{name}']")
    def test_private_map_contract(self):maps.validate_mobile(self.s)
    def test_no_vault_read(self):
        ET.SubElement(self.domain('offline_map'),'map',mr='vault_transfer',perms='r')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_no_raw_input(self):
        ET.SubElement(self.domain('offline_map'),'map',mr='input_handoff',perms='rw')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_no_network_channel(self):
        c=ET.SubElement(self.s,'channel');ET.SubElement(c,'end',pd='offline_map',id='3');ET.SubElement(c,'end',pd='browser_https',id='3')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_shell_cannot_write_map_pixels(self):
        self.ui.find("map[@mr='mobile_maps_frame']").set('perms','rw')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_browser_cannot_read_map_heap(self):
        ET.SubElement(self.domain('native_browser'),'map',mr='mobile_maps_heap',perms='r')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_shell_cannot_read_map_heap(self):
        ET.SubElement(self.ui,'map',mr='mobile_maps_heap',perms='r')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_shell_cannot_write_keyboard_status(self):
        self.ui.find("map[@mr='mobile_maps_state']").set('perms','rw')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_browser_cannot_read_map_keyboard_status(self):
        ET.SubElement(self.domain('native_browser'),'map',mr='mobile_maps_state',perms='r')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
if __name__=='__main__':unittest.main()
