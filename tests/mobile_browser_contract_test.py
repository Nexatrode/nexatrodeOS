"""Reject accidental app-data authority, writable results and UI map overlaps."""
import copy,importlib.util
from pathlib import Path
import unittest
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('mobile_builder',ROOT/'experiments/mobile-browser/build.py')
builder=importlib.util.module_from_spec(spec);spec.loader.exec_module(builder)

class Contracts(unittest.TestCase):
    def setUp(self):
        self.system=ET.Element('system')
        regions={'private_wallet':4096,'private_chat':4096,'private_vault':4096,'wallet_tcp_io':4096,'mobile_browser_frame':0x800000,'browser_heap':0x4000000,'mobile_browser_input':4096,'browser_request':4096,'browser_response':0x205000,'mobile_browser_state':4096,'browser_https_heap':0x1000000,'browser_tcp_io':4096,'rtc':4096}
        for name,size in regions.items():ET.SubElement(self.system,'memory_region',name=name,size=hex(size))
        def domain(name,permissions):
            d=ET.SubElement(self.system,'protection_domain',name=name)
            for i,(region,perms) in enumerate(permissions.items()):ET.SubElement(d,'map',mr=region,perms=perms,vaddr=hex(0x40000000+i*0x8000000))
        domain('native_browser',dict(mobile_browser_frame='rw',browser_heap='rw',mobile_browser_input='rw',browser_request='rw',browser_response='r',mobile_browser_state='rw'))
        domain('browser_https',dict(browser_https_heap='rw',browser_request='r',browser_response='rw',browser_tcp_io='rw',rtc='r'))
        domain('slint_ui',dict(mobile_browser_frame='r',mobile_browser_input='rw',mobile_browser_state='r'))
        domain('wallet_app',dict(private_wallet='rw',wallet_tcp_io='rw'))
        domain('simplex_agent',dict(private_chat='rw'));domain('messages_app',{});domain('vault',dict(private_vault='rw'))
    def mapped(self,pd,mr):return self.system.find(f"protection_domain[@name='{pd}']/map[@mr='{mr}']")
    def test_expected_contract(self):builder.validate(self.system)
    def test_extra_vault_access_is_rejected(self):
        d=self.system.find("protection_domain[@name='native_browser']");ET.SubElement(d,'map',mr='private_vault',vaddr='0x90000000',perms='r')
        with self.assertRaises(AssertionError):builder.validate(self.system)
    def test_ui_cannot_write_browser_surface(self):
        self.mapped('slint_ui','mobile_browser_frame').set('perms','rw')
        with self.assertRaises(AssertionError):builder.validate(self.system)
    def test_engine_cannot_write_https_response(self):
        self.mapped('native_browser','browser_response').set('perms','rw')
        with self.assertRaises(AssertionError):builder.validate(self.system)
    def test_broker_cannot_write_clock(self):
        self.mapped('browser_https','rtc').set('perms','rw')
        with self.assertRaises(AssertionError):builder.validate(self.system)
    def test_ui_overlap_is_rejected(self):
        self.mapped('slint_ui','mobile_browser_input').set('vaddr','0x40001000')
        with self.assertRaisesRegex(AssertionError,'overlapping mappings'):builder.validate(self.system)

if __name__=='__main__':unittest.main()
