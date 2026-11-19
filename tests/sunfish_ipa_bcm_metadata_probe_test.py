#!/usr/bin/env python3
"""Keep the IPA CMD_DB metadata probe read-only and route-scoped."""

from __future__ import annotations

import importlib.util
import re
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "rust/src/sunfish_ipa_bcm_metadata_probe.rs"
MANIFEST = ROOT / "platforms/sunfish/ipa-bcm-metadata-probe.system"
SPEC = importlib.util.spec_from_file_location(
    "sunfish_capdl_memory", ROOT / "platforms/sunfish/audit-capdl-memory.py"
)
AUDIT = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(AUDIT)


class IpaBcmMetadataProbeTests(unittest.TestCase):
    def test_only_reads_the_six_unresolved_ipa_route_records(self):
        source = SOURCE.read_text()
        resources = re.findall(r'^\s+"([A-Z0-9]+)",\s*$', source, re.MULTILINE)
        expected = ["SN1", "SH5", "SH2", "SN15", "CN0", "IP0"]
        self.assertEqual(resources, expected)
        self.assertIn("Metadata::from_resource", source)
        self.assertNotIn("Controller", source)
        self.assertNotIn("read_ipa_path_bcms", source)
        self.assertNotIn("apps_rsc", source)
        self.assertNotIn("qcom_rpmh_bus", source)

    def test_system_maps_only_read_only_cmd_db_and_reserved_display(self):
        root = ET.parse(MANIFEST).getroot()
        self.assertEqual([node.tag for node in root], ["memory_region"] * 3 + ["protection_domain"])
        expected = {
            "cmd_db_dictionary": (0x0C3F0000, 0x1000, "r"),
            "cmd_db_snapshot": (0x85F20000, 0x2000, "r"),
            "splash": (0x9C000000, 0x1600000, "rw"),
        }
        regions = {node.get("name"): node for node in root.findall("memory_region")}
        self.assertEqual(set(regions), set(expected))
        domain = root.find("protection_domain")
        self.assertNotEqual(domain.get("smc"), "true")
        mappings = {node.get("mr"): node for node in domain.findall("map")}
        self.assertEqual(set(mappings), set(expected))
        for name, (address, size, perms) in expected.items():
            self.assertEqual(int(regions[name].get("phys_addr"), 0), address)
            self.assertEqual(int(regions[name].get("size"), 0), size)
            self.assertEqual(mappings[name].get("perms"), perms)
            self.assertEqual(mappings[name].get("cached"), "false")
        self.assertEqual(
            AUDIT.read_only_cmd_db_pages(MANIFEST),
            {(0x85F20000, 0x1000), (0x85F21000, 0x1000)},
        )


if __name__ == "__main__":
    unittest.main()
