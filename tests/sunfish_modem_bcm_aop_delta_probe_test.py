import unittest
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class ModemBcmAopDeltaProbeTest(unittest.TestCase):
    def test_two_stable_read_series_bracket_the_single_aop_notification(self):
        source = (ROOT / "rust/src/sunfish_modem_bcm_aop_delta_probe.rs").read_text()
        before = source.index("let before = read_series")
        aop = source.index("pixel_aop_modem_on::run")
        after = source.index("let after = read_series")
        self.assertLess(before, aop)
        self.assertLess(aop, after)
        self.assertEqual(source.count("read_bcm_series("), 1)
        self.assertIn("if !before.passed()", source)
        self.assertIn("AOP ON BETWEEN READS", source)

    def test_delta_report_fails_closed_for_incomplete_or_unstable_samples(self):
        source = (ROOT / "rust/src/sunfish_modem_bcm_aop_delta_probe.rs").read_text()
        self.assertIn("fn stable_value", source)
        self.assertIn("fn pair_diff", source)
        self.assertIn("fn transition_diff", source)
        self.assertIn("_ => true", source)
        self.assertIn('"AOP DELTA MASK"', source)
        self.assertIn('"B PAIR DIFF"', source)
        self.assertIn('"A PAIR DIFF"', source)

    def test_probe_has_no_bcm_resource_write_or_modem_start_path(self):
        source = (ROOT / "rust/src/sunfish_modem_bcm_aop_delta_probe.rs").read_text()
        for forbidden in (
            "hold_proxy_resources",
            "hold_pas_bus_floor",
            "BcmWrite",
            "MdtTransfer",
            "release_modem_reset",
            "qmi_wlan",
            "dma_start",
        ):
            self.assertNotIn(forbidden, source)
        self.assertIn("NO BCM RESOURCE WRITE", source)
        self.assertIn("NO SECURE CALL", source)
        self.assertIn("NO MODEM OR WLAN START", source)

    def test_system_maps_only_resources_used_by_the_probe(self):
        tree = ET.parse(ROOT / "platforms/sunfish/modem-bcm-aop-delta-probe.system")
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


if __name__ == "__main__":
    unittest.main()
