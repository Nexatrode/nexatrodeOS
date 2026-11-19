"""The IPA route diagnostic may READ BCMs but receives no IPA or modem access."""

import importlib.util
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "platforms/sunfish/ipa-path-bcm-probe.system"
SPEC = importlib.util.spec_from_file_location(
    "sunfish_capdl_memory", ROOT / "platforms/sunfish/audit-capdl-memory.py")
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class IpaPathBcmProbeTests(unittest.TestCase):
    def test_only_cmd_db_rsc_and_display_windows_are_mapped(self):
        root = ET.parse(MANIFEST).getroot()
        expected = {
            "cmd_db_dictionary": (0x0C3F0000, 0x1000, 0x40000000, "r"),
            "cmd_db_snapshot": (0x85F20000, 0x2000, 0x40001000, "r"),
            "apps_rsc": (0x18220000, 0x1000, 0x40003000, "rw"),
            "apps_rsc_tail": (0x18221000, 0x2000, 0x40004000, "r"),
            "splash": (0x9C000000, 0x1600000, 0x42000000, "rw"),
        }
        regions = {node.get("name"): node for node in root.findall("memory_region")}
        self.assertEqual(set(regions), set(expected))
        pd = root.find("protection_domain")
        self.assertNotEqual(pd.get("smc"), "true")
        self.assertEqual([node.tag for node in pd], ["program_image"] + ["map"] * 5)
        mappings = {node.get("mr"): node for node in pd.findall("map")}
        self.assertEqual(set(mappings), set(expected))
        for name, (physical, size, virtual, perms) in expected.items():
            with self.subTest(name=name):
                self.assertEqual(int(regions[name].get("phys_addr"), 0), physical)
                self.assertEqual(int(regions[name].get("size"), 0), size)
                self.assertEqual(int(mappings[name].get("vaddr"), 0), virtual)
                self.assertEqual(mappings[name].get("perms"), perms)
                self.assertEqual(mappings[name].get("cached"), "false")
        self.assertEqual(root.findall("channel"), [])
        self.assertEqual(root.findall("memory_region[@name='ipa_mmio']"), [])

    def test_cmd_db_exception_is_limited_to_two_read_only_snapshot_pages(self):
        self.assertEqual(
            AUDIT.read_only_cmd_db_pages(MANIFEST),
            {(0x85F20000, 0x1000), (0x85F21000, 0x1000)},
        )

if __name__ == "__main__":
    unittest.main()
