import unittest
import re
from pathlib import Path
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[1]


class ModemBcmAfterAopProbeTest(unittest.TestCase):
    def test_reads_two_passes_after_aop_without_resource_write_path(self):
        source = (ROOT / "rust/src/sunfish_modem_bcm_after_aop_probe.rs").read_text()
        self.assertIn("pixel_aop_modem_on::run", source)
        self.assertIn("read_bcm_series", source)
        self.assertIn("pair_diff", source)
        self.assertIn("expected_diff", source)
        for forbidden in (
            "hold_proxy_resources",
            "hold_pas_bus_floor",
            "Mc0Reassert",
            "Sn11PasFloor",
            "Sn0PasFloor",
            "Ce0PasFloor",
        ):
            self.assertNotIn(forbidden, source)
        self.assertIn("NO BCM RESOURCE WRITE", source)
        self.assertIn("NO SECURE CALL", source)
        self.assertIn("NO MODEM OR WLAN START", source)

    def test_system_maps_only_the_required_probe_resources(self):
        tree = ET.parse(ROOT / "platforms/sunfish/modem-bcm-after-aop-probe.system")
        system = tree.getroot()
        regions = {node.attrib["name"]: node for node in system.findall("memory_region")}
        self.assertEqual(
            set(regions),
            {"apps_rsc", "apps_rsc_tail", "aop_msgram", "aop_doorbell", "pas_metadata", "splash"},
        )
        self.assertEqual(regions["apps_rsc"].attrib["phys_addr"], "0x18220000")
        self.assertEqual(regions["aop_msgram"].attrib["phys_addr"], "0x0c300000")
        self.assertEqual(regions["aop_doorbell"].attrib["phys_addr"], "0x17c00000")
        serialized = ET.tostring(system, encoding="unicode")
        self.assertNotIn("modem_pool", serialized)
        self.assertNotIn("wlan_msa", serialized)

    def test_difference_mask_bits_match_resource_index_and_screen_labels(self):
        read_source = (ROOT / "rust/src/qcom_rpmh_read.rs").read_text()
        resource_block = read_source.split("pub enum BcmResource {", 1)[1].split("}", 1)[0]
        resources = [
            line.strip().rstrip(",")
            for line in resource_block.splitlines()
            if line.strip() and not line.lstrip().startswith("//")
        ]
        self.assertEqual(resources, ["Mc0", "Acv", "Ce0", "Sn11", "Sn0", "Sh0"])

        source = (ROOT / "rust/src/sunfish_modem_bcm_after_aop_probe.rs").read_text()
        expected_block = source.split("const EXPECTED: [u32; 6] = [", 1)[1].split("]", 1)[0]
        expected = [
            int(value, 0)
            for value in re.findall(
                r"0x[0-9a-fA-F_]+|(?<![A-Za-z0-9_])0(?![A-Za-z0-9_])",
                expected_block,
            )
        ]
        self.assertEqual(
            expected,
            [0x29917F75, 0x20000008, 0, 0x213C44F1, 0x213C44F1, 0x24005ADF],
        )

        displayed = re.findall(
            r"\(BcmResource::(\w+), \[\"(\w+) PASS0\", \"(\w+) PASS1\"\]\)",
            source,
        )
        expected_display = [(name, name.upper(), name.upper()) for name in resources]
        self.assertEqual(displayed, expected_display)
        self.assertIn("r.responses[0][i] != r.responses[1][i]", source)
        self.assertIn("r.responses[pass][i] == Some(EXPECTED[i])", source)
        self.assertIn("<< i", source)


if __name__ == "__main__":
    unittest.main()
