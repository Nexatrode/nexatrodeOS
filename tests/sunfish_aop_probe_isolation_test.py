"""Check the MMIO capability boundaries of the two native AOP probes."""

import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class AopProbeIsolationTests(unittest.TestCase):
    def test_only_read_only_msgram_and_reserved_display_are_mapped(self):
        system = ET.parse(ROOT / "platforms/sunfish/aop-read-probe.system").getroot()
        self.assertEqual([node.tag for node in system],
                         ["memory_region", "memory_region", "protection_domain"])
        regions = {node.get("name"): node for node in system.findall("memory_region")}
        self.assertEqual(set(regions), {"aop_msgram", "splash"})
        self.assertEqual(regions["aop_msgram"].get("phys_addr"), "0x0c300000")
        self.assertEqual(regions["aop_msgram"].get("size"), "0x1000")
        self.assertEqual(regions["splash"].get("phys_addr"), "0x9c000000")
        self.assertEqual(regions["splash"].get("size"), "0x1600000")
        domain = system.find("protection_domain")
        self.assertIsNotNone(domain)
        self.assertNotEqual(domain.get("smc"), "true")
        self.assertEqual([node.tag for node in domain], ["program_image", "map", "map"])
        mappings = {node.get("mr"): node for node in domain.findall("map")}
        self.assertEqual(set(mappings), set(regions))
        self.assertEqual(mappings["aop_msgram"].get("perms"), "r")
        self.assertEqual(mappings["aop_msgram"].get("cached"), "false")
        self.assertEqual(mappings["splash"].get("perms"), "rw")
        self.assertEqual(system.findall("channel"), [])

    def test_handshake_only_adds_fixed_doorbell_and_msgram_write_permission(self):
        self.assert_handshake_resource_boundary("aop-handshake-probe.system")

    def test_packet_probe_adds_no_resources_beyond_handshake_probe(self):
        self.assert_handshake_resource_boundary("aop-packet-probe.system")

    def assert_handshake_resource_boundary(self, filename):
        system = ET.parse(ROOT / "platforms/sunfish" / filename).getroot()
        self.assertEqual([node.tag for node in system],
                         ["memory_region"] * 3 + ["protection_domain"])
        regions = {node.get("name"): node for node in system.findall("memory_region")}
        self.assertEqual(set(regions), {"aop_msgram", "aop_doorbell", "splash"})
        for name, address, size, page_size in [
            ("aop_msgram", "0x0c300000", "0x1000", "0x1000"),
            ("aop_doorbell", "0x17c00000", "0x1000", "0x1000"),
            ("splash", "0x9c000000", "0x1600000", "0x200000"),
        ]:
            self.assertEqual(regions[name].get("phys_addr"), address)
            self.assertEqual(regions[name].get("size"), size)
            self.assertEqual(regions[name].get("page_size"), page_size)
        domain = system.find("protection_domain")
        self.assertNotEqual(domain.get("smc"), "true")
        self.assertEqual([node.tag for node in domain], ["program_image"] + ["map"] * 3)
        mappings = {node.get("mr"): node for node in domain.findall("map")}
        self.assertEqual(set(mappings), set(regions))
        for mapping in mappings.values():
            self.assertEqual(mapping.get("perms"), "rw")
            self.assertEqual(mapping.get("cached"), "false")
        self.assertEqual(mappings["aop_msgram"].get("vaddr"), "0x40000000")
        self.assertEqual(mappings["aop_doorbell"].get("vaddr"), "0x40001000")
        self.assertEqual(system.findall("channel"), [])


if __name__ == "__main__":
    unittest.main()
