#!/usr/bin/env python3
"""Offline policy model for the observed Sunfish WLAN MSA handoff.

This module only validates and describes fixed ownership requests. It does
not call SCM, issue SMCs, change allocator state, or touch device memory.
"""

from __future__ import annotations

from dataclasses import dataclass


class MsaPolicyError(ValueError):
    """The pool or WLFW-reported regions do not satisfy the fixed policy."""


class MsaTransactionError(ValueError):
    """The offline assignment/rollback evidence is internally inconsistent."""


HLOS = 0x03
MSS_MSA = 0x0F
WLAN = 0x18
WLAN_CE = 0x19
READ_WRITE = 0x06
MAX_WLFW_REGIONS = 2


@dataclass(frozen=True)
class Region:
    base: int
    size: int
    secure_flag: int


def plan_msa_handoff(pool_base: int, pool_size: int, regions: list[Region]) -> dict:
    """Return a fixed-policy request plan for validated WLFW regions.

    The caller must still establish allocator exclusion, SMMU isolation, and
    an explicit secure-monitor ownership transition. Returned requests are
    descriptive data only and must never be executed by this module.
    """
    if pool_base < 0 or pool_size <= 0:
        raise MsaPolicyError("invalid reserved MSA pool")
    pool_end = pool_base + pool_size
    if pool_end <= pool_base:
        raise MsaPolicyError("MSA pool address overflow")
    if not regions or len(regions) > MAX_WLFW_REGIONS:
        raise MsaPolicyError("invalid WLFW region count")

    ordered = sorted(regions, key=lambda region: region.base)
    previous_end = pool_base
    requests = []
    for index, region in enumerate(ordered):
        if region.size <= 0 or region.base < pool_base:
            raise MsaPolicyError(f"region {index} is outside the reserved pool")
        region_end = region.base + region.size
        if region_end <= region.base or region_end > pool_end:
            raise MsaPolicyError(f"region {index} exceeds the reserved pool")
        if region.base < previous_end:
            raise MsaPolicyError(f"region {index} overlaps an earlier region")
        if region.secure_flag not in (0, 1):
            raise MsaPolicyError(f"region {index} has an unsupported secure flag")

        destinations = [
            {"vmid": MSS_MSA, "permissions": READ_WRITE},
            {"vmid": WLAN, "permissions": READ_WRITE},
        ]
        if region.secure_flag == 0:
            destinations.append({"vmid": WLAN_CE, "permissions": READ_WRITE})

        requests.append({
            "region_index": index,
            "base": region.base,
            "size": region.size,
            "source_vmids": [HLOS],
            "destinations": destinations,
        })
        previous_end = region_end

    return {
        "intent": "offline-plan-only",
        "executes_scm": False,
        "pool": {"base": pool_base, "size": pool_size},
        "requests": requests,
    }


def resolve_msa_transaction(
    region_count: int,
    *,
    assignment_verified: list[bool],
    rollback_verified_reverse: list[bool] | None = None,
) -> list[str]:
    """Resolve an offline model of per-region assignment and rollback.

    A true assignment/rollback value means the secure operation succeeded
    under a reviewed ABI whose return contract confirms the resulting owner;
    an independent owner readback may provide that evidence if one exists.
    False represents a failed or ambiguous result. This function does not
    perform either operation; it makes the fail-closed state policy testable.

    On the first unverified assignment, prior verified assignments are
    rolled back in reverse order. An unverified target or rollback is
    quarantined. Regions never attempted remain with their known HLOS owner.
    """
    if region_count <= 0 or len(assignment_verified) != region_count:
        raise MsaTransactionError("assignment evidence must cover every region")
    if any(type(value) is not bool for value in assignment_verified):
        raise MsaTransactionError("assignment evidence must be explicit booleans")

    first_failure = next(
        (index for index, verified in enumerate(assignment_verified) if not verified),
        None,
    )
    if first_failure is None:
        if rollback_verified_reverse not in (None, []):
            raise MsaTransactionError("rollback evidence supplied without a failure")
        return ["assigned-to-required-vmids"] * region_count

    completed_count = first_failure
    if rollback_verified_reverse is None or len(rollback_verified_reverse) != completed_count:
        raise MsaTransactionError("rollback evidence must cover prior assignments in reverse order")
    if any(type(value) is not bool for value in rollback_verified_reverse):
        raise MsaTransactionError("rollback evidence must be explicit booleans")

    states = ["known-hlos"] * region_count
    states[first_failure] = "quarantined-owner-unknown"
    # The evidence list is ordered newest successful region first.
    for region_index, rollback_ok in zip(
        range(first_failure - 1, -1, -1), rollback_verified_reverse
    ):
        states[region_index] = "known-hlos" if rollback_ok else "quarantined-owner-unknown"
    return states
