#!/usr/bin/env python3
"""Check that generated DMA addresses and the network client MAC agree."""
import importlib.util
from pathlib import Path
import struct
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('hvf_network_layout', ROOT / 'tools/check-hvf-network-layout.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class NetworkLayoutTest(unittest.TestCase):
    def setUp(self):
        self.root = ET.fromstring('''<system>
          <memory_region name="virtio_mmio@a000000/drivers/network/virtio/mmio/hw_ring_buffer" phys_addr="0x1000" />
          <memory_region name="virtio_mmio@a000000/net/rx/data/device" phys_addr="0x2000" />
          <memory_region name="virtio_mmio@a000000/net/tx/data/client/dhcp_client" phys_addr="0x3000" />
        </system>''')
        self.sections = {k: bytearray(n) for k, n in [('device',56),('rx',140),('tx',112),('client',128)]}
        for key, offset, value in [('device',48,0x1000),('rx',64,0x2000),('tx',104,0x3000)]:
            struct.pack_into('<Q', self.sections[key], offset, value)
        self.sections['rx'][128:134] = self.sections['client'][120:126] = bytes.fromhex('1e4dd2275052')

    def test_agree(self):
        self.assertEqual(module.validate(self.root, self.sections), [])

    def test_mismatch_detected(self):
        struct.pack_into('<Q', self.sections['device'], 48, 0x9000)
        self.sections['client'][120] ^= 1
        errors = module.validate(self.root, self.sections)
        self.assertEqual(len(errors), 2)
        self.assertTrue(any('MAC' in e for e in errors))
        self.assertTrue(any('ring' in e for e in errors))

if __name__ == '__main__': unittest.main()
