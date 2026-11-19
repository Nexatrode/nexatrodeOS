"""RSC handoff inspection must have no platform write or start authority."""
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class HandoffProbeIsolationTests(unittest.TestCase):
    def test_exact_read_only_controller_window_and_reserved_splash_only(self):
        root = ET.parse(ROOT / "platforms/sunfish/rpmh-handoff-probe.system").getroot()
        self.assertEqual([n.tag for n in root], ["memory_region"] * 2 + ["protection_domain"])
        expected = {
            "apps_rsc_state": (0x18220000, 0x3000, "r"),
            "splash": (0x9c000000, 0x1600000, "rw"),
        }
        regions = {n.get("name"): n for n in root.findall("memory_region")}
        domain = root.find("protection_domain")
        self.assertNotEqual(domain.get("smc"), "true")
        self.assertEqual([n.tag for n in domain], ["program_image"] + ["map"] * 2)
        mappings = {n.get("mr"): n for n in domain.findall("map")}
        self.assertEqual(set(regions), set(expected))
        self.assertEqual(set(mappings), set(expected))
        for name, (address, size, perms) in expected.items():
            self.assertEqual(int(regions[name].get("phys_addr"), 0), address)
            self.assertEqual(int(regions[name].get("size"), 0), size)
            self.assertEqual(mappings[name].get("perms"), perms)
            self.assertEqual(mappings[name].get("cached"), "false")


if __name__ == "__main__":
    unittest.main()
