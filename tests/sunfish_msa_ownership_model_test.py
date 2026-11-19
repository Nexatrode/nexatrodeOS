#!/usr/bin/env python3
"""Host-only tests for the fixed Sunfish MSA handoff policy."""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "platforms" / "sunfish"))

from msa_ownership_model import (  # noqa: E402
    MsaPolicyError,
    MsaTransactionError,
    Region,
    plan_msa_handoff,
    resolve_msa_transaction,
)


MANIFEST = ROOT / "platforms" / "sunfish" / "wlan-resources.json"


class MsaOwnershipModelTests(unittest.TestCase):
    def setUp(self) -> None:
        doc = json.loads(MANIFEST.read_text())
        msa = doc["icnss"]["msa"]
        cells = msa["fixed_region_reg_cells"]
        self.pool_base = int(cells[1], 0)
        self.pool_size = int(cells[3], 0)
        self.regions = [
            Region(int(item["base"], 0), int(item["size"], 0), item["secure_flag"])
            for item in msa["observed_wlfw_segments"]
        ]

    def test_observed_ranges_produce_fixed_secure_and_nonsecure_targets(self) -> None:
        plan = plan_msa_handoff(self.pool_base, self.pool_size, self.regions)
        self.assertFalse(plan["executes_scm"])
        self.assertEqual(plan["intent"], "offline-plan-only")
        self.assertEqual(
            [(r["base"], r["size"]) for r in plan["requests"]],
            [(0x98D00000, 0x4000), (0x98D04000, 0x1FC000)],
        )
        self.assertEqual(plan["requests"][0]["source_vmids"], [0x03])
        self.assertEqual(
            plan["requests"][0]["destinations"],
            [{"vmid": 0x0F, "permissions": 0x06}, {"vmid": 0x18, "permissions": 0x06}],
        )
        self.assertEqual(
            plan["requests"][1]["destinations"],
            [
                {"vmid": 0x0F, "permissions": 0x06},
                {"vmid": 0x18, "permissions": 0x06},
                {"vmid": 0x19, "permissions": 0x06},
            ],
        )

    def test_rejects_region_outside_reserved_pool(self) -> None:
        bad = [Region(self.pool_base - 0x1000, 0x1000, 1)]
        with self.assertRaisesRegex(MsaPolicyError, "outside the reserved pool"):
            plan_msa_handoff(self.pool_base, self.pool_size, bad)

    def test_rejects_region_overflowing_reserved_pool(self) -> None:
        bad = [Region(self.pool_base + self.pool_size - 0x1000, 0x2000, 0)]
        with self.assertRaisesRegex(MsaPolicyError, "exceeds the reserved pool"):
            plan_msa_handoff(self.pool_base, self.pool_size, bad)

    def test_rejects_overlapping_wlfw_regions(self) -> None:
        bad = [Region(self.pool_base, 0x2000, 1), Region(self.pool_base + 0x1000, 0x1000, 0)]
        with self.assertRaisesRegex(MsaPolicyError, "overlaps"):
            plan_msa_handoff(self.pool_base, self.pool_size, bad)

    def test_rejects_unknown_flag_and_region_count(self) -> None:
        with self.assertRaisesRegex(MsaPolicyError, "unsupported secure flag"):
            plan_msa_handoff(self.pool_base, self.pool_size, [Region(self.pool_base, 0x1000, 2)])
        with self.assertRaisesRegex(MsaPolicyError, "region count"):
            plan_msa_handoff(self.pool_base, self.pool_size, self.regions + [self.regions[0]])

    def test_verified_assignments_leave_every_region_with_required_vmids(self) -> None:
        self.assertEqual(
            resolve_msa_transaction(2, assignment_verified=[True, True]),
            ["assigned-to-required-vmids", "assigned-to-required-vmids"],
        )

    def test_partial_assignment_rolls_back_in_reverse_and_quarantines_unknown(self) -> None:
        states = resolve_msa_transaction(
            2,
            assignment_verified=[True, False],
            rollback_verified_reverse=[True],
        )
        self.assertEqual(states, ["known-hlos", "quarantined-owner-unknown"])

    def test_failed_rollback_quarantines_previously_assigned_region(self) -> None:
        states = resolve_msa_transaction(
            3,
            assignment_verified=[True, True, False],
            rollback_verified_reverse=[False, True],
        )
        self.assertEqual(
            states,
            ["known-hlos", "quarantined-owner-unknown", "quarantined-owner-unknown"],
        )

    def test_rejects_incomplete_or_inconsistent_transaction_evidence(self) -> None:
        with self.assertRaisesRegex(MsaTransactionError, "rollback evidence"):
            resolve_msa_transaction(2, assignment_verified=[True, False])
        with self.assertRaisesRegex(MsaTransactionError, "explicit booleans"):
            resolve_msa_transaction(1, assignment_verified=[1])  # type: ignore[list-item]


if __name__ == "__main__":
    unittest.main()
