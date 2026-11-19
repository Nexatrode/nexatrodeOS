#!/usr/bin/env python3
"""Host-only policy tests; these do not touch Pixel hardware."""

from __future__ import annotations

import copy
import json
import re
import sys
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "platforms" / "sunfish"))

from smmu_handoff_model import (  # noqa: E402
    HandoffError,
    decode_global_cfg0,
    decode_mapping_group_count,
    plan_wlan_route,
)


MANIFEST = ROOT / "platforms" / "sunfish" / "wlan-resources.json"
SEL4_CONFIG = ROOT / "platforms" / "sunfish" / "sel4-config.cmake"
SMMU_PROBE_SYSTEM = ROOT / "platforms" / "sunfish" / "smmu-read-probe.system"
SMMU_PROBE_SOURCE = ROOT / "platforms" / "sunfish" / "smmu-read-probe.c"
RUST_SMMU_SOURCE = ROOT / "rust" / "src" / "sunfish_smmu.rs"


class HandoffModelTests(unittest.TestCase):
    def setUp(self) -> None:
        document = json.loads(MANIFEST.read_text())
        self.snapshot = copy.deepcopy(document["smmu_handoff_snapshot"])

    def plan(self, **overrides: object) -> dict:
        args: dict = {
            "smmu_control_delegated": True,
            "ownership_transferred": True,
            "route_snapshot_fresh_and_complete": True,
            "verified_free_slots": [6],
            "verified_free_context_banks": [4],
        }
        args.update(overrides)
        return plan_wlan_route(self.snapshot, **args)

    def test_requires_explicit_ownership_transfer(self) -> None:
        with self.assertRaisesRegex(HandoffError, "ownership transfer"):
            self.plan(ownership_transferred=False)

    def test_requires_fresh_complete_route_table_not_targeted_readback(self) -> None:
        with self.assertRaisesRegex(HandoffError, "fresh complete SMMU route-table"):
            self.plan(route_snapshot_fresh_and_complete=False)

    def test_requires_explicit_control_delegation_separate_from_ownership(self) -> None:
        with self.assertRaisesRegex(HandoffError, "control delegation to EL1"):
            self.plan(smmu_control_delegated=False)

    def test_sunfish_config_keeps_generic_destructive_smmu_disabled(self) -> None:
        config = SEL4_CONFIG.read_text()
        self.assertIn("set(KernelArmSMMU OFF)", config)

    def test_does_not_infer_free_context_bank(self) -> None:
        with self.assertRaisesRegex(HandoffError, "verified free context bank"):
            self.plan(verified_free_context_banks=[])

    def test_rejects_a_slot_already_used_by_handoff(self) -> None:
        with self.assertRaisesRegex(HandoffError, "verified free stream-match slot"):
            self.plan(verified_free_slots=[0])

    def test_plan_preserves_observed_routes_and_adds_only_the_pair(self) -> None:
        before = copy.deepcopy(self.snapshot["observed_valid_routes"])
        result = self.plan()
        self.assertEqual(result["existing_routes_after"], before)
        self.assertEqual(result["matched_sids"], [0x240, 0x241])
        self.assertEqual(result["add_route"]["slot"], 6)
        self.assertEqual(result["add_route"]["id"], 0x240)
        self.assertEqual(result["add_route"]["mask"], 0x1)
        self.assertEqual(result["add_route"]["context_bank"], 4)
        self.assertEqual(result["add_route"]["s2cr_type"], "translation")
        self.assertTrue(result["add_route"]["context_translation_enabled"])
        self.assertEqual(
            result["sid_slot_bindings"],
            [
                {"sid": 0x240, "match_slot": 6},
                {"sid": 0x241, "match_slot": 6},
            ],
        )
        self.assertNotEqual(result["add_route"]["slot"], 0x240)
        self.assertFalse(result["writes_hardware"])
        self.assertEqual(self.snapshot["observed_valid_routes"], before)

    def test_mapping_group_count_matches_raw_id0_numsmrg(self) -> None:
        measured_count = decode_mapping_group_count(
            int(self.snapshot["id0_register_value"], 0)
        )
        self.assertEqual(measured_count, 78)
        source = RUST_SMMU_SOURCE.read_text()
        match = re.search(
            r"pub const SUNFISH_MAX_SMR_SLOTS: usize = (\d+);", source
        )
        self.assertIsNotNone(match, "Rust route planner is missing its SMR limit")
        self.assertEqual(int(match.group(1)), measured_count)
        self.snapshot["mapping_group_count"] = 79
        with self.assertRaisesRegex(HandoffError, "disagrees with ID0.NUMSMRG"):
            self.plan()

    def test_rejects_invalid_id0_register_value(self) -> None:
        for value in (-1, 0x1_0000_0000, True):
            with self.subTest(value=value):
                with self.assertRaisesRegex(HandoffError, "must be a u32"):
                    decode_mapping_group_count(value)  # type: ignore[arg-type]

    def test_rejects_candidate_that_collides_with_observed_route(self) -> None:
        self.snapshot["observed_valid_routes"].append(
            {
                "slot": 6,
                "id": "0x0240",
                "mask": "0x0001",
                "s2cr_type": 0,
                "context_bank": 4,
            }
        )
        with self.assertRaisesRegex(HandoffError, "overlaps observed slot 6"):
            self.plan(verified_free_slots=[7], verified_free_context_banks=[5])

    def test_rejects_duplicate_observed_slots(self) -> None:
        self.snapshot["observed_valid_routes"].append(
            copy.deepcopy(self.snapshot["observed_valid_routes"][0])
        )
        with self.assertRaisesRegex(HandoffError, "duplicate slots"):
            self.plan()

    def test_rejects_out_of_range_observed_route(self) -> None:
        self.snapshot["observed_valid_routes"][0]["slot"] = 78
        with self.assertRaisesRegex(HandoffError, "slot is outside"):
            self.plan()

    def test_rejects_malformed_candidate_or_bad_free_resource_assertion(self) -> None:
        self.snapshot["candidate_wlan_match"]["mask"] = "not-a-mask"
        with self.assertRaisesRegex(HandoffError, "malformed candidate"):
            self.plan()

        self.setUp()
        with self.assertRaisesRegex(HandoffError, "context bank is outside"):
            self.plan(verified_free_context_banks=[61])

        with self.assertRaisesRegex(HandoffError, "stream-match slot is outside"):
            self.plan(verified_free_slots=[78])

    def test_high_stream_id_uses_bounded_hardware_slot(self) -> None:
        result = self.plan(verified_free_slots=[77])
        self.assertEqual(result["add_route"]["slot"], 77)
        self.assertEqual(result["sid_slot_bindings"][0]["sid"], 0x240)
        self.assertEqual(result["sid_slot_bindings"][0]["match_slot"], 77)

    def test_large_sid_width_does_not_require_scanning_the_full_sid_space(self) -> None:
        self.snapshot["stream_id_width_bits"] = 32
        self.snapshot["candidate_wlan_match"]["mask"] = "0xffffffff"
        with self.assertRaisesRegex(HandoffError, "declared SID set"):
            self.plan()

    def test_rejects_incomplete_snapshot_and_non_mapping_route(self) -> None:
        del self.snapshot["context_bank_count"]
        with self.assertRaisesRegex(HandoffError, "snapshot is malformed"):
            self.plan()

        self.setUp()
        self.snapshot["observed_valid_routes"][0] = None
        with self.assertRaisesRegex(HandoffError, "route entry is malformed"):
            self.plan()

    def test_rejects_unknown_observed_s2cr_type(self) -> None:
        self.snapshot["observed_valid_routes"][0]["s2cr_type"] = 3
        with self.assertRaisesRegex(HandoffError, "unknown S2CR type"):
            self.plan()


class HandoffProbeCapabilityTests(unittest.TestCase):
    def test_decodes_observed_global_cfg0_common_bits_without_claiming_bank(self) -> None:
        decoded = decode_global_cfg0(0x002A0406)
        self.assertEqual(
            decoded,
            {
                "client_accesses_processed": True,
                "global_fault_report_enabled": True,
                "global_fault_interrupt_enabled": True,
                "unidentified_streams_fault": True,
                "memory_type_configured": False,
                "bypass_memattr": None,
                "stream_match_conflicts_fault": True,
            },
        )
        self.assertEqual(
            decode_global_cfg0(0x002A0407)["client_accesses_processed"], False
        )

    def test_rejects_non_u32_global_cfg0(self) -> None:
        for value in (-1, 0x1_0000_0000, True):
            with self.subTest(value=value):
                with self.assertRaisesRegex(HandoffError, "must be a u32"):
                    decode_global_cfg0(value)  # type: ignore[arg-type]

    def test_abl_probe_reads_global_cfg0_and_ufs_sid_without_smmu_write_authority(self) -> None:
        system = ET.parse(SMMU_PROBE_SYSTEM).getroot()
        smmu = system.find("memory_region[@name='apps_smmu']")
        probe = system.find("protection_domain[@name='smmu_read_probe']")
        self.assertIsNotNone(smmu)
        self.assertIsNotNone(probe)
        assert smmu is not None and probe is not None
        self.assertEqual(smmu.attrib["phys_addr"], "0x15000000")
        self.assertEqual(probe.attrib["smc"], "false")
        self.assertFalse(
            any(pd.attrib.get("smc") == "true" for pd in system.findall("protection_domain"))
        )
        self.assertIsNone(system.find("protection_domain[@name='smmu_read_watchdog']"))
        mapping = probe.find("map[@mr='apps_smmu']")
        self.assertIsNotNone(mapping)
        assert mapping is not None
        self.assertEqual(mapping.attrib["perms"], "r")
        self.assertEqual(mapping.attrib["cached"], "false")
        writable_maps = [
            entry.attrib["mr"]
            for entry in probe.findall("map")
            if "w" in entry.attrib.get("perms", "")
        ]
        self.assertEqual(writable_maps, ["splash"])

        source = SMMU_PROBE_SOURCE.read_text()
        self.assertIn("SMMU_GLOBAL_CFG0 0x000u", source)
        self.assertIn('key_hex("SMMU GR0 CFG0", cfg0)', source)
        self.assertIn("0x300u", source)

    def test_full_route_table_and_context_state_fit_the_read_only_screen(self) -> None:
        source = SMMU_PROBE_SOURCE.read_text()
        self.assertIn("#define SMMU_CB_ACTLR 0x004u", source)
        self.assertIn("read32(base + SMMU_CB_ACTLR)", source)
        self.assertIn('append_text64(out, &at, " Y")', source)
        self.assertIn('make_mask_line(mask_lines[3], "CB00 TO CB3F ACTLR NZ"', source)
        self.assertIn('make_metric_line(metric_lines[9], "ACTLR NONZERO"', source)
        self.assertNotIn("write32(", source)
        self.assertIn("#define MAX_DISPLAY_ROUTES 80u", source)
        self.assertIn('"S SLOT SID MASK T CB CBAR SCTLR ACTLR"', source)
        self.assertIn('"T0 TRANS T1 BYPASS T2 FAULT T3 OTHER"', source)
        self.assertIn('append_text64(out, &at, " A")', source)
        self.assertIn('append_text64(out, &at, " X")', source)
        self.assertIn('make_mask_line(mask_lines[0], "CB00 TO CB3F ROUTE"', source)
        self.assertIn('make_mask_line(mask_lines[1], "CB00 TO CB3F BAR NZ"', source)
        self.assertIn('make_mask_line(mask_lines[2], "CB00 TO CB3F SCTLR M"', source)

        # 80 rows cover the 78-slot Sunfish table with a small margin.
        last_route_bottom = 626 + (80 - 1) * 21 + (7 * 2)
        self.assertLess(last_route_bottom, 2340)

    def test_probe_shows_progress_before_mmio_and_between_scan_phases(self) -> None:
        source = SMMU_PROBE_SOURCE.read_text()
        markers = [
            'draw_checkpoint("READ START")',
            'uint32_t cfg0 = read32(SMMU_GLOBAL_CFG0)',
            'draw_checkpoint("GLOBAL READ DONE")',
            'draw_checkpoint("ROUTE SCAN DONE")',
            'draw_checkpoint("CONTEXT SCAN DONE")',
            "draw_report(cfg0, global_fsr, id0, id1, num_cb)",
        ]
        offsets = [source.index(marker) for marker in markers]
        self.assertEqual(offsets, sorted(offsets))
        self.assertIn("static void draw_checkpoint(const char *text)", source)
        self.assertIn("volatile uint32_t *frame", source)
        self.assertIn('draw_text_scaled(frame, 40u, 72u, text, white, 4u)', source)


if __name__ == "__main__":
    unittest.main()
