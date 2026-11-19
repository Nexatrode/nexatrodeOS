#!/usr/bin/env python3
"""Check recorded ABL SMMU snapshots for WLAN SID collisions and evidence.

This does not inspect live hardware and does not prove ownership of an SMMU
stream-match slot or context bank.
"""

from __future__ import annotations

import json
from pathlib import Path

from smmu_handoff_model import (
    HandoffError,
    decode_global_cfg0,
    decode_mapping_group_count,
)


MANIFEST = Path(__file__).with_name("wlan-resources.json")


def matches(sid: int, route_id: int, mask: int, width_mask: int) -> bool:
    """Return whether an SID matches an ARM SMMU stream-match pattern."""
    return ((sid ^ route_id) & ~mask & width_mask) == 0


def overlaps(id_a: int, mask_a: int, id_b: int, mask_b: int, width_mask: int) -> bool:
    """Return whether two wildcard patterns share at least one SID."""
    significant = ~(mask_a | mask_b) & width_mask
    return ((id_a ^ id_b) & significant) == 0


def check(snapshot: dict) -> list[str]:
    errors: list[str] = []
    width = snapshot["stream_id_width_bits"]
    width_mask = (1 << width) - 1
    count_smes = snapshot["mapping_group_count"]
    try:
        id0_value = int(snapshot["id0_register_value"], 0)
        decoded_group_count = decode_mapping_group_count(id0_value)
        if count_smes != decoded_group_count:
            errors.append("mapping-group count disagrees with ID0.NUMSMRG")
    except (KeyError, TypeError, ValueError, HandoffError):
        errors.append("SMMU ID0.NUMSMRG evidence is missing or invalid")
    count_cbs = snapshot["context_bank_count"]
    stage2_only = snapshot["stage2_only_context_bank_count"]
    sctlr_enabled = snapshot["translation_enabled_context_banks"]
    if not 0 <= sctlr_enabled <= count_cbs:
        errors.append("translation-enabled context-bank count is outside the bank count")
    if not 0 <= stage2_only <= count_cbs:
        errors.append("stage-2-only context-bank count is outside the bank count")
    if snapshot["page_size_bytes"] not in (4096, 65536):
        errors.append("recorded context-bank page size is not supported by this check")
    candidate = snapshot["candidate_wlan_match"]
    candidate_id = int(candidate["id"], 0)
    candidate_mask = int(candidate["mask"], 0)
    expected_ids = sorted(int(sid, 0) for sid in candidate["matched_ids"])
    actual_ids = sorted(
        sid for sid in range(width_mask + 1)
        if matches(sid, candidate_id, candidate_mask, width_mask)
    )
    if expected_ids != actual_ids:
        errors.append(
            f"candidate SID set is {actual_ids!r}, manifest says {expected_ids!r}"
        )

    routes = snapshot["observed_valid_routes"]
    slots: set[int] = set()
    for route in routes:
        slot = route["slot"]
        route_id = int(route["id"], 0)
        route_mask = int(route["mask"], 0)
        cb = route["context_bank"]
        route_type = route["s2cr_type"]
        if route_type not in (0, 1, 2):
            errors.append(f"slot {slot} has unknown S2CR type {route_type}")
        if slot in slots:
            errors.append(f"duplicate observed stream-match slot {slot}")
        slots.add(slot)
        if not 0 <= slot < count_smes:
            errors.append(f"observed slot {slot} is outside {count_smes} mapping groups")
        if not 0 <= cb < count_cbs:
            errors.append(f"observed context bank {cb} is outside {count_cbs} banks")
        if route_id & ~width_mask or route_mask & ~width_mask:
            errors.append(f"slot {slot} uses bits outside the {width}-bit SID space")
        for sid in actual_ids:
            if matches(sid, route_id, route_mask, width_mask):
                errors.append(f"candidate SID 0x{sid:04x} matches observed slot {slot}")
        if overlaps(candidate_id, candidate_mask, route_id, route_mask, width_mask):
            errors.append(f"candidate wildcard pattern overlaps observed slot {slot}")

    return errors


def check_latest_targeted(readback: dict) -> tuple[list[str], int, bool]:
    """Validate the newer targeted screen without treating it as a full table."""
    errors: list[str] = []
    try:
        global_register = readback["global_register_0"]
        value = int(global_register["value"], 0)
        decoded = decode_global_cfg0(value)
        targets = readback["target_sids"]
        ufs = readback["context_bank_2_user_transcription"]
        cbar = int(ufs["cbar"], 0)
        sctlr = int(ufs["sctlr"], 0)
        ttbr0 = int(ufs["ttbr0"], 0)
        tcr = int(ufs["tcr"], 0)
    except (KeyError, TypeError, ValueError, HandoffError) as exc:
        return [f"malformed latest targeted SMMU readback: {exc}"], 0, False

    expected_wlan = ["0x240", "0x241", "0x520", "0x521", "0x522"]
    no_route_count = 0
    if not isinstance(targets, dict):
        errors.append("latest targeted SMMU SID list is malformed")
    else:
        for sid in expected_wlan:
            if targets.get(sid) == "no matching route displayed":
                no_route_count += 1
            else:
                errors.append(f"latest targeted readback is missing no-route result for {sid}")
        if not str(targets.get("0x300", "")).startswith(
            "one matching TRANS route to CB 2"
        ):
            errors.append("latest targeted readback is missing the reported UFS CB2 route")
    expected_common_fields = {
        "CLIENTPD_bit0": value & 1,
        "GFRE_bit1": (value >> 1) & 1,
        "GFIE_bit2": (value >> 2) & 1,
        "USFCFG_bit10": (value >> 10) & 1,
        "MTCFG_bit20": (value >> 20) & 1,
        "SMCFCFG_bit21": (value >> 21) & 1,
    }
    if global_register.get("decoded_common_fields") != expected_common_fields:
        errors.append("latest global-register bit transcription does not match its value")
    if decoded["client_accesses_processed"] is not True:
        errors.append("latest CFG0 value has client accesses disabled")
    if decoded["global_fault_report_enabled"] is not True:
        errors.append("latest CFG0 value does not enable global fault reporting")
    if decoded["global_fault_interrupt_enabled"] is not True:
        errors.append("latest CFG0 value does not enable global fault interrupts")
    if decoded["unidentified_streams_fault"] is not True:
        errors.append("latest CFG0 value does not enable unidentified-stream faults")
    if decoded["memory_type_configured"] is not False:
        errors.append("latest CFG0 value unexpectedly enables MemAttr configuration")
    if decoded["stream_match_conflicts_fault"] is not True:
        errors.append("latest CFG0 value does not fault stream-match conflicts")
    if cbar != 0x0001F000:
        errors.append("reported CB2 CBAR differs from the saved transcription")
    if sctlr != 0x000000E0 or sctlr & 1:
        errors.append("reported CB2 SCTLR differs from the non-translating transcription")
    if ttbr0 != 0:
        errors.append("reported CB2 TTBR0 differs from the saved zero transcription")
    if tcr != 0:
        errors.append("reported CB2 TCR differs from the saved zero transcription")
    bank = global_register.get("bank_identity")
    if not isinstance(bank, str) or "not confirmed" not in bank:
        errors.append("latest CFG0 readback must retain its unconfirmed bank identity")
    return errors, no_route_count, decoded["unidentified_streams_fault"]


def main() -> int:
    try:
        manifest = json.loads(MANIFEST.read_text())
        snapshot = manifest["smmu_handoff_snapshot"]
        latest = snapshot["latest_targeted_readback_2026_10_21"]
    except (OSError, json.JSONDecodeError, KeyError) as exc:
        print(f"FAIL: cannot load SMMU handoff policy: {exc}")
        return 1

    errors = check(snapshot)
    latest_errors, no_route_count, unknown_stream_fault = check_latest_targeted(latest)
    errors.extend(latest_errors)
    if errors:
        print("FAIL: recorded ABL WLAN SID collision check")
        for error in errors:
            print(f"  - {error}")
        return 1

    matched = ", ".join(snapshot["candidate_wlan_match"]["matched_ids"])
    type_names = {0: "translation", 1: "bypass", 2: "fault"}
    type_counts = {route_type: 0 for route_type in type_names}
    for route in snapshot["observed_valid_routes"]:
        type_counts[route["s2cr_type"]] += 1
    type_summary = ", ".join(
        f"{type_counts[route_type]} {name}"
        for route_type, name in type_names.items()
    )
    print(
        "OK: candidate WLAN match covers "
        f"{matched} and does not overlap {len(snapshot['observed_valid_routes'])} "
        f"recorded ABL patterns ({type_summary}); SCTLR.M is enabled in "
        f"{snapshot['translation_enabled_context_banks']}/{snapshot['context_bank_count']} "
        "context banks, but free-bank ownership is still unproven"
    )
    print(
        "Latest targeted readback: no route for "
        f"{no_route_count} WLAN SIDs; unidentified-stream fault="
        f"{unknown_stream_fault}; CB2 SCTLR.M=0; register bank and free-resource "
        "ownership remain unproven"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
