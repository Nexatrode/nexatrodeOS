#!/usr/bin/env python3
"""Source-level check that generic SMC authority stays in reset watchdog."""

from __future__ import annotations

import unittest
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SYSTEM = ROOT / "platforms" / "sunfish" / "slint-display.system"
TOUCH_SOURCE = ROOT / "platforms" / "sunfish" / "touch-service.c"


class SlintSmcPolicyTests(unittest.TestCase):
    def test_only_fixed_purpose_reset_domain_receives_smc_capability(self) -> None:
        system = ET.parse(SYSTEM).getroot()
        enabled = [
            pd.get("name")
            for pd in system.findall("protection_domain")
            if pd.get("smc", "false").lower() == "true"
        ]
        self.assertEqual(enabled, ["slint_reset"])

    def test_touch_driver_does_not_issue_generic_smc(self) -> None:
        source = TOUCH_SOURCE.read_text()
        self.assertNotIn("microkit_arm_smc_call", source)
        self.assertNotIn("PSCI_SYSTEM_RESET", source)


if __name__ == "__main__":
    unittest.main()
