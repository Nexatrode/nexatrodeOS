"""Saved-place plaintext/key stays in Maps; only ciphertext crosses to store."""
import importlib.util,unittest
from pathlib import Path
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('private_maps_adapter',ROOT/'experiments/desktop-apps/maps.py')
maps=importlib.util.module_from_spec(spec);spec.loader.exec_module(maps)
class PrivateAuthority(unittest.TestCase):
    def setUp(self):
        self.s=ET.Element('system')
        for n in ('slint_ui','vault','wallet_app','simplex_agent','native_browser','browser_https','entropy_rng','timer_driver'):
            ET.SubElement(self.s,'protection_domain',name=n,priority='130' if n=='timer_driver' else '121' if n=='entropy_rng' else '100')
        def mr(name,size,**kw):ET.SubElement(self.s,'memory_region',name=name,size=hex(size),**kw)
        def mapping(pd,name,va,perms,symbol):ET.SubElement(pd,'map',mr=name,vaddr=hex(va),perms=perms,setvar_vaddr=symbol)
        def channel(a,i,b,j,pp=False):
            c=ET.SubElement(self.s,'channel');ET.SubElement(c,'end',pd=a,id=str(i),**({'pp':'true'} if pp else {}));ET.SubElement(c,'end',pd=b,id=str(j))
        maps.attach_mobile(self.s,self.pd('slint_ui'),mapping,mr,channel,storage=True,private=True)
    def pd(self,name):return self.s.find(f"protection_domain[@name='{name}']")
    def test_isolated_ciphertext_and_entropy(self):maps.validate_mobile(self.s)
    def test_no_shell_ciphertext_or_key_mapping(self):
        ET.SubElement(self.pd('slint_ui'),'map',mr='map_private_cipher',perms='r')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_no_browser_private_data_mapping(self):
        ET.SubElement(self.pd('native_browser'),'map',mr='mobile_maps_heap',perms='r')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_no_shared_vault_key_or_namespace_capability(self):
        c=ET.SubElement(self.s,'channel');ET.SubElement(c,'end',pd='offline_map',id='5');ET.SubElement(c,'end',pd='vault',id='31')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
    def test_no_raw_disk_access_by_renderer(self):
        ET.SubElement(self.pd('offline_map'),'map',mr='map_disk_regs',perms='rw')
        with self.assertRaises(AssertionError):maps.validate_mobile(self.s)
if __name__=='__main__':unittest.main()
