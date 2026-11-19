#!/usr/bin/env python3
"""Tests for measured LineageOS SMMU route diagnostics; no device access."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "platforms" / "sunfish"))

from smmu_runtime_route_readback import (  # noqa: E402
    RouteReadbackError,
    parse_route_readback,
    require_route,
    require_usb_context_reassignment,
)


CAPTURE = ROOT / "platforms/sunfish/fixtures/lineage-smmu-runtime-route-readback.log"


class RuntimeRouteReadbackTests(unittest.TestCase):
    def setUp(self) -> None:
        self.records = parse_route_readback(CAPTURE.read_text())

    def test_icnss_alias_route_matches_raw_smr_and_s2cr(self) -> None:
        require_route(
            self.records,
            sid=0x240,
            mask=0x1,
            device="18800000.qcom,icnss",
            slot=13,
            context_bank=6,
            cbar=0x0001F300,
            sctlr=0x00C000E6,
        )
        route = next(item for item in self.records if item.sid == 0x240)
        self.assertEqual(route.smr, 0x80010240)
        self.assertEqual(route.s2cr, 0x00000306)
        self.assertEqual(route.sctlr & 1, 0)

    def test_ipa_wlan_is_a_distinct_exact_sid_route(self) -> None:
        require_route(
            self.records,
            sid=0x521,
            mask=0,
            device="soc:ipa_smmu_wlan",
            slot=9,
            context_bank=38,
            cbar=0x0001F300,
            sctlr=0x00C000E6,
        )
        self.assertFalse(any(item.sid == 0x521 and item.mask == 1 for item in self.records))

    def test_ipa_ap_and_uc_routes_remain_separate(self) -> None:
        for sid, device, slot, bank in (
            (0x520, "soc:ipa_smmu_ap", 8, 37),
            (0x522, "soc:ipa_smmu_uc", 10, 39),
        ):
            with self.subTest(sid=sid):
                require_route(
                    self.records,
                    sid=sid,
                    mask=0,
                    device=device,
                    slot=slot,
                    context_bank=bank,
                    cbar=0x0001F300,
                    sctlr=0x00C000E6,
                )

    def test_usb_context_reassignment_is_not_collapsed(self) -> None:
        require_usb_context_reassignment(self.records)
        usb = [item.context_bank for item in self.records if item.sid == 0x540]
        self.assertEqual(usb, [1, 23, 23, 23, 23, 23])

    def test_raw_route_mismatch_is_rejected(self) -> None:
        text = CAPTURE.read_text().replace("s2cr=0x00000306", "s2cr=0x00000307", 1)
        with self.assertRaisesRegex(RouteReadbackError, "disagrees with S2CR"):
            parse_route_readback(text)

    def test_empty_capture_is_rejected(self) -> None:
        with self.assertRaisesRegex(RouteReadbackError, "contains no SUNFISH_SMMU"):
            parse_route_readback("ordinary kernel log line\n")


if __name__ == "__main__":
    unittest.main()
