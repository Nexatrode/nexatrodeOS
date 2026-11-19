"""The CX read transaction must not acquire modem, other-bank or DMA mappings."""

import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class CxReadIsolationTests(unittest.TestCase):
    def test_one_owner_has_only_the_first_apps_rsc_page_and_splash(self):
        for filename in ["rpmh-cx-read-probe.system", "rpmh-resource-reads-probe.system",
                         "rpmh-cx-turbo-hold-probe.system", "rpmh-resources-hold-probe.system"]:
            with self.subTest(manifest=filename):
                self.check_manifest(filename)

    def check_manifest(self, filename):
        root = ET.parse(ROOT / "platforms/sunfish" / filename).getroot()
        self.assertEqual([n.tag for n in root], ["memory_region"] * 2 + ["protection_domain"])
        expected = {"apps_rsc": (0x18220000, 0x1000, 0x1000),
                    "splash": (0x9c000000, 0x1600000, 0x200000)}
        regions = {n.get("name"): n for n in root.findall("memory_region")}
        self.assertEqual(set(regions), set(expected))
        for name, values in expected.items():
            self.assertEqual(tuple(int(regions[name].get(prop), 0)
                                   for prop in ["phys_addr", "size", "page_size"]), values)
        domain = root.find("protection_domain")
        self.assertNotEqual(domain.get("smc"), "true")
        self.assertEqual([n.tag for n in domain], ["program_image", "map", "map"])
        self.assertEqual({n.get("mr") for n in domain.findall("map")}, set(expected))
        for mapping in domain.findall("map"):
            self.assertEqual(mapping.get("perms"), "rw")
            self.assertEqual(mapping.get("cached"), "false")
        # The first sleep TCS starts beyond the mapped page (0xd00 + 2*0x2a0).
        self.assertGreaterEqual(0xd00 + 2 * 0x2a0, 0x1000)


if __name__ == "__main__":
    unittest.main()
