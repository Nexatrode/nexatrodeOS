"""Map app authority must remain private to its window and region."""
import importlib.util,unittest
from pathlib import Path
import xml.etree.ElementTree as ET
spec=importlib.util.spec_from_file_location('maps_adapter',Path(__file__).resolve().parents[1]/'experiments/desktop-apps/maps.py')
adapter=importlib.util.module_from_spec(spec);spec.loader.exec_module(adapter)
class MapAuthority(unittest.TestCase):
    def setUp(self):
        self.s=ET.Element('system');self.domains={}
        for n in ('offline_map','desktop_manager','desktop_gpu','chat_ui','wallet_ui','native_browser','wallet_app','simplex_agent','vault'):
            self.domains[n]=ET.SubElement(self.s,'protection_domain',name=n)
        for mr,perms in {'map_frame':'rw','map_heap':'rw','map_input':'rw','map_observation':'rw','desktop_display_mode':'r'}.items():self.map('offline_map',mr,perms)
        self.map('desktop_manager','map_frame','r');self.map('desktop_gpu','map_frame','r')
        for i,j in [(0,29),(1,28)]:
            c=ET.SubElement(self.s,'channel');ET.SubElement(c,'end',pd='offline_map',id=str(i));ET.SubElement(c,'end',pd='desktop_manager',id=str(j))
    def map(self,pd,mr,perms):return ET.SubElement(self.domains[pd],'map',mr=mr,perms=perms)
    def test_private_window_contract(self):adapter.validate(self.s)
    def test_no_raw_input_or_global_framebuffer(self):
        self.map('offline_map','input_handoff','rw')
        with self.assertRaises(AssertionError):adapter.validate(self.s)
    def test_no_wallet_data(self):
        self.map('offline_map','wallet_transfer','r')
        with self.assertRaises(AssertionError):adapter.validate(self.s)
    def test_no_network_endpoint(self):
        c=ET.SubElement(self.s,'channel');ET.SubElement(c,'end',pd='offline_map',id='3');ET.SubElement(c,'end',pd='network',id='9')
        with self.assertRaises(AssertionError):adapter.validate(self.s)
    def test_no_cross_app_map_input(self):
        self.map('chat_ui','map_input','r')
        with self.assertRaises(AssertionError):adapter.validate(self.s)
    def test_gpu_cannot_write_map_frame(self):
        self.domains['desktop_gpu'].find('map').set('perms','rw')
        with self.assertRaises(AssertionError):adapter.validate(self.s)
    def test_fail_closed_for_unintegrated_lock_profile(self):
        ET.SubElement(self.s,'protection_domain',name='lock_auth')
        with self.assertRaises(AssertionError):adapter.validate(self.s)
if __name__=='__main__':unittest.main()
