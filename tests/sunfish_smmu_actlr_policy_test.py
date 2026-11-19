#!/usr/bin/env python3
"""Host-only checks for the source-backed Sunfish WLAN ACTLR policy."""

from __future__ import annotations

import copy
import importlib.util
import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VALIDATOR_PATH = ROOT / "platforms" / "sunfish" / "validate-wlan-resources.py"
MANIFEST_PATH = ROOT / "platforms" / "sunfish" / "wlan-resources.json"
PLANNER_PATH = ROOT / "rust" / "src" / "sunfish_smmu_program.rs"
SPEC = importlib.util.spec_from_file_location("validate_wlan_resources", VALIDATOR_PATH)
assert SPEC and SPEC.loader
VALIDATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VALIDATOR)


class SunfishSmmuActlrPolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.manifest = json.loads(MANIFEST_PATH.read_text())

    def test_pinned_source_matches_route_and_lifecycle_inventory(self) -> None:
        self.assertEqual(VALIDATOR.check_wlan_smmu_actlr_policy(self.manifest), [])

    def test_qcom_smr_matching_covers_the_masked_wlan_alias_pair(self) -> None:
        entry = (0x240, 0x1, 0x103)
        self.assertTrue(VALIDATOR.actlr_entry_matches_route(entry, 0x240, 0x1))
        self.assertTrue(VALIDATOR.actlr_entry_matches_route(entry, 0x241, 0x1))
        self.assertFalse(VALIDATOR.actlr_entry_matches_route(entry, 0x242, 0x1))

    def test_current_sunfish_actlr_table_has_no_wlan_match(self) -> None:
        policy = self.manifest["wlan_smmu_actlr_policy"]
        entries = [
            tuple(int(value, 0) for value in row)
            for row in policy["apps_smmu_actlr_entries"]
        ]
        route = policy["route"]
        aliases = [int(value, 0) for value in route["matched_stream_ids"]]
        mask = int(route["stream_mask"], 0)
        matches = [
            entry
            for entry in entries
            if any(VALIDATOR.actlr_entry_matches_route(entry, sid, mask) for sid in aliases)
        ]
        self.assertEqual(matches, [])

    def test_fresh_linux_context_default_is_distinct_from_live_hardware_state(self) -> None:
        behavior = self.manifest["wlan_smmu_actlr_policy"]["source_derived_behavior"]
        self.assertEqual(behavior["fresh_context_actlr"], "0x00000000")
        self.assertIn("newly zero-allocated software", behavior["fresh_context_actlr_scope"])
        planner = PLANNER_PATH.read_text()
        match = re.search(
            r"pub const SUNFISH_WLAN_FRESH_UNMATCHED_ACTLR: u32 = (0x[0-9a-fA-F]+|[0-9]+);",
            planner,
        )
        self.assertIsNotNone(match)
        assert match is not None
        self.assertEqual(int(match.group(1), 0), int(behavior["fresh_context_actlr"], 0))
        self.assertTrue(behavior["apps_smmu_skip_init"])
        self.assertEqual(behavior["live_context_actlr"], "unknown")
        self.assertFalse(behavior["zero_is_safe_runtime_default"])

    def test_policy_rejects_a_nonzero_fresh_linux_context_claim(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["wlan_smmu_actlr_policy"]["source_derived_behavior"][
            "fresh_context_actlr"
        ] = "0x00000103"
        errors = VALIDATOR.check_wlan_smmu_actlr_policy(manifest)
        self.assertTrue(any("fresh zero-allocated" in error for error in errors))

    def test_policy_rejects_claiming_zero_as_the_live_or_safe_value(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        policy = manifest["wlan_smmu_actlr_policy"]["source_derived_behavior"]
        policy["live_context_actlr"] = "0"
        policy["zero_is_safe_runtime_default"] = True
        errors = VALIDATOR.check_wlan_smmu_actlr_policy(manifest)
        self.assertTrue(any("must not claim zero" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
