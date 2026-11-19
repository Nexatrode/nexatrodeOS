"""The RPMh observation must not acquire platform write capabilities."""

import importlib.util
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "platforms/sunfish/rpmh-read-probe.system"
SPEC = importlib.util.spec_from_file_location(
    "sunfish_capdl_memory", ROOT / "platforms/sunfish/audit-capdl-memory.py")
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class RpmhReadIsolationTests(unittest.TestCase):
    def test_only_fixed_metadata_pages_and_reserved_display_are_mapped(self):
        root = ET.parse(MANIFEST).getroot()
        self.assertEqual([n.tag for n in root], ["memory_region"] * 4 + ["protection_domain"])
        expected = {
            "cmd_db_dictionary": (0x0c3f0000, 0x1000, "r"),
            "cmd_db_snapshot": (0x85f20000, 0x2000, "r"),
            "apps_rsc_state": (0x18220000, 0x1000, "r"),
            "splash": (0x9c000000, 0x1600000, "rw"),
        }
        regions = {n.get("name"): n for n in root.findall("memory_region")}
        self.assertEqual(set(regions), set(expected))
        domain = root.find("protection_domain")
        self.assertNotEqual(domain.get("smc"), "true")
        self.assertEqual([n.tag for n in domain], ["program_image"] + ["map"] * 4)
        mappings = {n.get("mr"): n for n in domain.findall("map")}
        self.assertEqual(set(mappings), set(expected))
        for name, (address, size, perms) in expected.items():
            self.assertEqual(int(regions[name].get("phys_addr"), 0), address)
            self.assertEqual(int(regions[name].get("size"), 0), size)
            self.assertEqual(mappings[name].get("perms"), perms)
            self.assertEqual(mappings[name].get("cached"), "false")
        self.assertEqual(root.findall("channel"), [])

    def test_memory_audit_has_no_default_xbl_exception(self):
        self.assertEqual(AUDIT.read_only_cmd_db_pages(None), set())
        self.assertEqual(AUDIT.read_only_cmd_db_pages(MANIFEST),
                         {(0x85f20000, 0x1000), (0x85f21000, 0x1000)})

    def check_refused(self, mutate):
        root = ET.parse(MANIFEST).getroot()
        mutate(root)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "probe.system"
            ET.ElementTree(root).write(path)
            with self.assertRaises(ValueError):
                AUDIT.read_only_cmd_db_pages(path)

    def test_writable_or_cached_database_cannot_use_the_exception(self):
        self.check_refused(lambda r: r.find(".//map[@mr='cmd_db_dictionary']").set("perms", "rw"))
        self.check_refused(lambda r: r.find(".//map[@mr='cmd_db_dictionary']").set("cached", "true"))
        self.check_refused(lambda r: r.find(".//map[@mr='cmd_db_snapshot']").set("perms", "rw"))
        self.check_refused(lambda r: r.find(".//map[@mr='cmd_db_snapshot']").set("cached", "true"))

    def test_other_xbl_pages_and_writable_aliases_cannot_use_the_exception(self):
        self.check_refused(lambda r: r.find("memory_region[@name='cmd_db_dictionary']").set("phys_addr", "0x0c3f1000"))
        self.check_refused(lambda r: ET.SubElement(r.find("protection_domain"), "map",
            {"mr": "cmd_db_dictionary", "vaddr": "0x50000000", "perms": "rw", "cached": "false"}))
        self.check_refused(lambda r: r.find("memory_region[@name='cmd_db_snapshot']").set("phys_addr", "0x85f10000"))
        self.check_refused(lambda r: r.find("memory_region[@name='cmd_db_snapshot']").set("size", "0x3000"))
        self.check_refused(lambda r: ET.SubElement(r.find("protection_domain"), "map",
            {"mr": "cmd_db_snapshot", "vaddr": "0x50000000", "perms": "rw", "cached": "false"}))


if __name__ == "__main__":
    unittest.main()
