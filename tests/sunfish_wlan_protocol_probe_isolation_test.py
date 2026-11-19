#!/usr/bin/env python3
"""Keep the on-device WLAN protocol probe isolated from radio hardware."""

from __future__ import annotations

import unittest
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SYSTEM = ROOT / "platforms/sunfish/wlan-protocol-probe.system"
PROBE = ROOT / "rust/src/wlan_protocol_probe.rs"


class WlanProtocolProbeIsolationTests(unittest.TestCase):
    def test_probe_has_only_the_reserved_splash_mapping(self) -> None:
        system = ET.parse(SYSTEM).getroot()
        self.assertEqual([node.tag for node in system], ["memory_region", "protection_domain"])

        region, domain = system
        self.assertEqual(region.attrib["name"], "splash")
        self.assertEqual(region.attrib["phys_addr"], "0x9c000000")
        self.assertEqual(region.attrib["size"], "0x1600000")
        self.assertEqual(domain.attrib["name"], "wlan_protocol_probe")
        self.assertNotEqual(domain.attrib.get("smc"), "true")
        self.assertEqual(len(domain.findall("program_image")), 1)
        self.assertEqual(len(domain.findall("map")), 1)
        mapping = domain.find("map")
        assert mapping is not None
        self.assertEqual(mapping.attrib["mr"], "splash")
        self.assertEqual(mapping.attrib["perms"], "rw")
        self.assertEqual(domain.findall("irq"), [])
        self.assertEqual(domain.findall("channel"), [])

    def test_probe_executes_start_and_stop_scan_through_bounded_transport_model(self) -> None:
        source = PROBE.read_text()
        for marker in (
            "fn scan_transfer_codec_ok() -> bool",
            "StartScanRequest::linux_reference_defaults(1, &channels, &ssids, &ies)",
            "WmiCommandTxQueue::<4>::new_wmi_control(",
            "queue_start_scan(",
            "queue_stop_scan(",
            "WMI_10_4_STOP_SCAN_COMMAND_ID",
            "0x0030",
            "queue.reclaim_to(2, &mut completed)",
            "draw_text(\n                frame,\n                if scan_tx_ok",
        ):
            self.assertIn(marker, source)
        self.assertNotIn("CeTxRing", source)
        self.assertNotIn("Wcn3990RegisterIo", source)

    def test_probe_exercises_only_synthetic_key_frames_and_security_indications(self) -> None:
        source = PROBE.read_text()
        for marker in (
            "fn key_frame_codec_ok() -> bool",
            "let key = [0x5a; CCMP128_KEY_BYTES]",
            "CcmpKeyInstallTracker::new(",
            "queue_ccmp_install_key(",
            "HTT_10_4_T2H_SECURITY_INDICATION",
            "tracker.observe_security_indication(",
            "IgnoredStaleGeneration",
            "all_required_installed: true",
            "queue.reclaim_to(2, &mut completed)",
            '"KEY FRAME PASS"',
            '"NO RADIO ACCESS"',
        ):
            self.assertIn(marker, source)
        self.assertNotIn("CeTxRing", source)
        self.assertNotIn("Wcn3990RegisterIo", source)


if __name__ == "__main__":
    unittest.main()
