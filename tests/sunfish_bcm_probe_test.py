"""Bus metadata inspection must not acquire controller or modem authority."""
import importlib.util
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "platforms/sunfish/bcm-read-probe.system"
SPEC = importlib.util.spec_from_file_location(
    "sunfish_capdl_memory", ROOT / "platforms/sunfish/audit-capdl-memory.py")
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class BcmProbeIsolationTests(unittest.TestCase):
    def test_only_database_read_pages_and_reserved_display_are_mapped(self):
        root = ET.parse(MANIFEST).getroot()
        self.assertEqual([node.tag for node in root], ["memory_region"] * 3 + ["protection_domain"])
        expected = {
            "cmd_db_dictionary": (0x0c3f0000, 0x1000, "r"),
            "cmd_db_snapshot": (0x85f20000, 0x2000, "r"),
            "splash": (0x9c000000, 0x1600000, "rw"),
        }
        regions = {node.get("name"): node for node in root.findall("memory_region")}
        self.assertEqual(set(regions), set(expected))
        domain = root.find("protection_domain")
        self.assertNotEqual(domain.get("smc"), "true")
        self.assertEqual([node.tag for node in domain], ["program_image"] + ["map"] * 3)
        mappings = {node.get("mr"): node for node in domain.findall("map")}
        self.assertEqual(set(mappings), set(expected))
        for name, (address, size, perms) in expected.items():
            self.assertEqual(int(regions[name].get("phys_addr"), 0), address)
            self.assertEqual(int(regions[name].get("size"), 0), size)
            self.assertEqual(mappings[name].get("perms"), perms)
            self.assertEqual(mappings[name].get("cached"), "false")
        self.assertEqual(AUDIT.read_only_cmd_db_pages(MANIFEST),
                         {(0x85f20000, 0x1000), (0x85f21000, 0x1000)})


if __name__ == "__main__":
    unittest.main()
