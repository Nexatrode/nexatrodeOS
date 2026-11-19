#!/usr/bin/env python3
"""Offline, fail-closed model for planning a Sunfish WLAN SMMU route.

This is not a register driver and does not inspect or modify hardware. The
caller must supply a fresh snapshot plus resources already proven free under
an explicit ownership transfer.
"""

from __future__ import annotations

from copy import deepcopy


class HandoffError(ValueError):
    """The supplied snapshot or ownership evidence is insufficient."""


def decode_mapping_group_count(id0_value: int) -> int:
    """Decode ID0.NUMSMRG as the raw number of mapping groups.

    The pinned Sunfish Linux driver uses this field directly as an array
    length; it is not an encoded count-minus-one value.
    """
    if type(id0_value) is not int or not 0 <= id0_value <= 0xFFFF_FFFF:
        raise HandoffError("SMMU ID0 value must be a u32")
    return id0_value & 0xFF


def matches(sid: int, route_id: int, mask: int, width_mask: int) -> bool:
    return ((sid ^ route_id) & ~mask & width_mask) == 0


def overlaps(id_a: int, mask_a: int, id_b: int, mask_b: int, width_mask: int) -> bool:
    significant = ~(mask_a | mask_b) & width_mask
    return ((id_a ^ id_b) & significant) == 0


def decode_global_cfg0(value: int) -> dict[str, object]:
    """Decode SMMUv2 CFG0 fields common to the secure and non-secure banks.

    This does not identify which bank was read. The register bank is selected
    by the access security state, and callers must retain that uncertainty.
    """
    if type(value) is not int or not 0 <= value <= 0xFFFF_FFFF:
        raise HandoffError("global CFG0 value must be a u32")
    return {
        "client_accesses_processed": (value & 1) == 0,
        "global_fault_report_enabled": bool(value & (1 << 1)),
        "global_fault_interrupt_enabled": bool(value & (1 << 2)),
        "unidentified_streams_fault": bool(value & (1 << 10)),
        "memory_type_configured": bool(value & (1 << 20)),
        "bypass_memattr": ((value >> 16) & 0xF) if value & (1 << 20) else None,
        "stream_match_conflicts_fault": bool(value & (1 << 21)),
    }


def plan_wlan_route(
    snapshot: dict,
    *,
    smmu_control_delegated: bool = False,
    ownership_transferred: bool,
    route_snapshot_fresh_and_complete: bool,
    verified_free_slots: list[int],
    verified_free_context_banks: list[int],
) -> dict:
    """Return a proposed route update without mutating the snapshot.

    ``smmu_control_delegated`` is a separate platform assertion that EL1 is
    authorized to program this SMMU. A resource-ownership claim alone cannot
    establish that privilege boundary.
    ``verified_free_*`` are assertions from a trusted ownership/readback flow;
    this host model cannot establish them. No default or inferred free resource
    is accepted.
    """
    if route_snapshot_fresh_and_complete is not True:
        raise HandoffError("fresh complete SMMU route-table evidence is required")
    if smmu_control_delegated is not True:
        raise HandoffError("explicit SMMU control delegation to EL1 is required")
    if ownership_transferred is not True:
        raise HandoffError("explicit SMMU ownership transfer is required")
    required = {
        "stream_id_width_bits",
        "id0_register_value",
        "mapping_group_count",
        "context_bank_count",
        "candidate_wlan_match",
        "observed_valid_routes",
    }
    if not isinstance(snapshot, dict) or not required.issubset(snapshot):
        raise HandoffError("SMMU snapshot is malformed")

    width = snapshot["stream_id_width_bits"]
    if type(width) is not int or not 0 < width <= 32:
        raise HandoffError("invalid stream-ID width")
    width_mask = (1 << width) - 1
    group_count = snapshot["mapping_group_count"]
    bank_count = snapshot["context_bank_count"]
    if type(group_count) is not int or group_count <= 0:
        raise HandoffError("invalid stream-match group count")
    try:
        id0_value = int(snapshot["id0_register_value"], 0)
    except (TypeError, ValueError) as error:
        raise HandoffError("invalid SMMU ID0 register value") from error
    if group_count != decode_mapping_group_count(id0_value):
        raise HandoffError("mapping-group count disagrees with ID0.NUMSMRG")
    if type(bank_count) is not int or bank_count <= 0:
        raise HandoffError("invalid context-bank count")
    candidate = snapshot["candidate_wlan_match"]
    if not isinstance(candidate, dict) or not isinstance(candidate.get("matched_ids"), list):
        raise HandoffError("malformed candidate WLAN stream match")
    try:
        route_id = int(candidate["id"], 0)
        route_mask = int(candidate["mask"], 0)
        expected_ids = sorted(int(value, 0) for value in candidate["matched_ids"])
    except (KeyError, TypeError, ValueError) as error:
        raise HandoffError("malformed candidate WLAN stream match") from error
    if not expected_ids or len(set(expected_ids)) != len(expected_ids):
        raise HandoffError("candidate WLAN stream match has an invalid declared SID set")
    if route_id & ~width_mask or route_mask & ~width_mask or route_id & route_mask:
        raise HandoffError("candidate WLAN stream match does not match the declared SID set")
    expected_match_count = 1 << bin(route_mask).count("1")
    if len(expected_ids) != expected_match_count or any(
        sid & ~width_mask or not matches(sid, route_id, route_mask, width_mask)
        for sid in expected_ids
    ):
        raise HandoffError("candidate WLAN stream match does not match the declared SID set")
    actual_ids = expected_ids

    routes = snapshot["observed_valid_routes"]
    if not isinstance(routes, list):
        raise HandoffError("observed route table is malformed")
    used_slots: set[int] = set()
    used_banks: set[int] = set()
    for route in routes:
        if not isinstance(route, dict):
            raise HandoffError("observed route entry is malformed")
        try:
            slot = route["slot"]
            bank = route["context_bank"]
            route_type = route["s2cr_type"]
            existing_id = int(route["id"], 0)
            existing_mask = int(route["mask"], 0)
        except (KeyError, TypeError, ValueError) as error:
            raise HandoffError("observed route entry is malformed") from error
        if type(slot) is not int or not 0 <= slot < group_count:
            raise HandoffError("observed route slot is outside the hardware range")
        if slot in used_slots:
            raise HandoffError("observed route table contains duplicate slots")
        if type(bank) is not int or not 0 <= bank < bank_count:
            raise HandoffError("observed route context bank is outside the hardware range")
        if type(route_type) is not int or route_type not in (0, 1, 2):
            raise HandoffError("observed route has an unknown S2CR type")
        if existing_id & ~width_mask or existing_mask & ~width_mask:
            raise HandoffError("observed route stream match exceeds the stream-ID width")
        used_slots.add(slot)
        used_banks.add(bank)
        if overlaps(route_id, route_mask, existing_id, existing_mask, width_mask):
            raise HandoffError(f"candidate stream match overlaps observed slot {slot}")

    if not isinstance(verified_free_slots, list) or any(
        type(slot) is not int or not 0 <= slot < group_count
        for slot in verified_free_slots
    ):
        raise HandoffError("verified stream-match slot is outside the hardware range")
    if not isinstance(verified_free_context_banks, list) or any(
        type(bank) is not int or not 0 <= bank < bank_count
        for bank in verified_free_context_banks
    ):
        raise HandoffError("verified context bank is outside the hardware range")

    free_slots = sorted(set(verified_free_slots) - used_slots)
    free_banks = sorted(set(verified_free_context_banks) - used_banks)
    if not free_slots:
        raise HandoffError("no explicitly verified free stream-match slot")
    if not free_banks:
        raise HandoffError("no explicitly verified free context bank")
    slot, bank = free_slots[0], free_banks[0]

    # Preserve the original table byte-for-byte at the data level and return a
    # separate proposed addition. A later hardware driver still needs a
    # race-free readback/compare-and-update protocol.
    return {
        "existing_routes_after": deepcopy(routes),
        "add_route": {
            "slot": slot,
            "id": route_id,
            "mask": route_mask,
            "s2cr_type": "translation",
            "context_bank": bank,
            "context_translation_enabled": True,
        },
        "matched_sids": actual_ids,
        # Keep kernel-visible SID identities separate from the bounded
        # hardware SMR/S2CR slot. Multiple SIDs covered by one masked match
        # intentionally reference the same slot.
        "sid_slot_bindings": [
            {"sid": sid, "match_slot": slot} for sid in actual_ids
        ],
        "writes_hardware": False,
    }
