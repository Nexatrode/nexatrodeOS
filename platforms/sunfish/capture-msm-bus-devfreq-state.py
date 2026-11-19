#!/usr/bin/env python3
"""Capture read-only qcom,devbw state from a connected Sunfish over ADB.

This reads the devfreq class only. It does not restart adbd as root, write
sysfs, change a governor, enable logging, or alter bus/RPMh state.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_INVENTORY = ROOT / ".cache/pixel-sunfish/msm-bus-client-inventory-current-20261023-fabric.json"


def parse_integer(raw: str) -> int:
    return int(raw.strip(), 0)


def parse_integer_list(raw: str) -> list[int]:
    return [int(token, 0) for token in raw.split()]


def summarize_client(
    profile: dict,
    attributes: dict[str, str | None],
    value_semantics: dict | None = None,
) -> dict:
    opps = profile["operating_points_v2"]["entries"]
    active_opps = {
        entry["request_ib_mibps"]
        for entry in opps
        if entry["status"] not in ("disabled", '"disabled"')
    }
    errors = {
        name: value
        for name, value in attributes.items()
        if value is None
    }
    governor = attributes.get("governor")
    available_raw = attributes.get("available_frequencies")
    current_raw = attributes.get("cur_freq")
    available = parse_integer_list(available_raw) if available_raw is not None else None
    current = parse_integer(current_raw) if current_raw is not None else None
    governor_matches = governor == profile["governor"] if governor is not None else None
    current_matches_table = current in active_opps if current is not None else None
    current_is_available = (
        current in available if current is not None and available is not None else None
    )
    current_ib = (
        current
        if current_matches_table and current_is_available
        else None
    )
    semantics = value_semantics or {}
    zero_average_governors = semantics.get("zero_average_governors", [])
    average_known = (
        governor_matches is True and governor in zero_average_governors
    )
    average_mibps = 0 if average_known else None
    vote_unknowns = []
    if current_ib is None:
        vote_unknowns.append("current cur_freq is not confirmed in both the enabled OPP table and available_frequencies")
    if not average_known:
        vote_unknowns.append("average-bandwidth gov_ab is dynamic or its governor is not source-matched")
    paths = profile.get("paths", [])
    if not paths:
        vote_unknowns.append("client has no route paths in the captured inventory")
    path_count = len(paths)
    active_only = bool(profile.get("active_only", False))
    average_bps_per_path = (
        average_mibps // path_count * (1 << 20)
        if average_mibps is not None and path_count
        else None
    )
    peak_bps = current_ib * (1 << 20) if current_ib is not None else None
    route_contributions = []
    for path in paths:
        route = path.get("route")
        if not route or path.get("route_error"):
            vote_unknowns.append(f"path {path.get('path_index')} has no source-resolved route")
        contexts = {
            "active_only": {
                "average": average_bps_per_path if active_only else 0,
                "peak": peak_bps if active_only else 0,
            },
            "dual": {
                "average": 0 if active_only else average_bps_per_path,
                "peak": 0 if active_only else peak_bps,
            },
        }
        route_contributions.append({
            "path_index": path.get("path_index"),
            "source_id": path.get("source_id"),
            "destination_id": path.get("destination_id"),
            "route": route,
            "bcms": path.get("bcms"),
            "contexts": contexts,
        })
    return {
        "node": profile["node"],
        "runtime_name": profile["runtime_name"],
        "sysfs_path": f"/sys/class/devfreq/{profile['runtime_name']}",
        "expected_governor": profile["governor"],
        "observed_governor": governor,
        "governor_matches_profile": governor_matches,
        "available_frequencies": available,
        "available_frequencies_match_opp_table": (
            bool(available) and all(value in active_opps for value in available)
            if available is not None else None
        ),
        "cur_freq": current,
        "cur_freq_is_available": current_is_available,
        "cur_freq_matches_opp_table": current_matches_table,
        "current_ib_mibps": current_ib,
        "opp_table_node": profile["operating_points_v2"]["node"],
        "opp_ib_mibps": sorted(active_opps),
        "active_only": active_only,
        "path_count": path_count,
        "average_request_mibps": average_mibps,
        "average_request_source": (
            semantics.get("zero_average_source") if average_known
            else semantics.get("dynamic_average_source") if governor_matches is True
            else "observed governor is not source-matched to the inventory"
        ),
        "average_request_model": (
            semantics.get("dynamic_average_model")
            if not average_known and governor_matches is True
            and governor in semantics.get("dynamic_average_governors", [])
            else None
        ),
        "route_contributions": route_contributions,
        "request_profile_status": "complete" if not vote_unknowns else "partial",
        "request_profile_unknowns": sorted(set(vote_unknowns)),
        "status": "complete" if not errors else "partial",
        "unreadable_attributes": errors,
    }


def parse_devices(raw: str) -> list[tuple[str, str]]:
    rows = []
    for line in raw.splitlines()[1:]:
        fields = line.split()
        if len(fields) >= 2:
            rows.append((fields[0], fields[1]))
    return rows


def adb_text(*args: str, check: bool = True) -> str:
    result = subprocess.run(
        ["adb", *args], capture_output=True, text=True, check=False
    )
    if check and result.returncode:
        raise RuntimeError(result.stderr.strip() or f"adb {' '.join(args)} failed")
    return result.stdout.strip() if result.returncode == 0 else ""


def read_attribute(serial: str, sysfs_path: str, attribute: str) -> str | None:
    result = subprocess.run(
        ["adb", "-s", serial, "exec-out", "cat", f"{sysfs_path}/{attribute}"],
        capture_output=True,
        text=True,
        check=False,
    )
    # Toybox/ADB can report a remote `cat` failure on stdout while `adb
    # exec-out` itself exits zero. Treat both remote diagnostics and stderr as
    # unreadable data instead of letting the numeric decoder parse `cat:`.
    output = result.stdout.strip()
    if result.returncode or result.stderr.strip() or output.startswith("cat:"):
        return None
    return output or None


def capture(inventory: dict) -> dict:
    profiles = inventory.get("devbw_clients", [])
    if not profiles:
        raise ValueError("inventory contains no devbw clients")
    value_semantics = inventory.get("devbw_value_semantics")
    if not value_semantics or not value_semantics.get("zero_average_governors"):
        raise ValueError("inventory lacks source-validated devbw value semantics")
    for profile in profiles:
        if not profile.get("runtime_name") or not profile.get("operating_points_v2"):
            raise ValueError(f"incomplete devbw profile: {profile.get('node', '<unknown>')}")

    devices = parse_devices(adb_text("devices", "-l"))
    if len(devices) != 1 or devices[0][1] != "device":
        raise RuntimeError(f"expected exactly one authorized ADB device, found {devices}")
    serial = devices[0][0]
    product = adb_text("-s", serial, "shell", "getprop", "ro.product.device")
    if product != "sunfish":
        raise RuntimeError(f"refusing capture: expected Sunfish, found {product!r}")

    rows = []
    for profile in profiles:
        sysfs = f"/sys/class/devfreq/{profile['runtime_name']}"
        attributes = {
            name: read_attribute(serial, sysfs, name)
            for name in ("governor", "cur_freq", "available_frequencies")
        }
        rows.append(summarize_client(profile, attributes, value_semantics))
    return {
        "captured_at_utc": datetime.now(timezone.utc).isoformat(),
        "product": product,
        "scope": (
            "Read-only devfreq sysfs snapshot for DT-declared qcom,devbw clients. "
            "It combines source-confirmed zero AB for performance/powersave with "
            "selected IB OPPs where available. Dynamic bw_vbif AB, bus-node "
            "aggregates, and committed RPMh commands remain outside this capture."
        ),
        "dtb_sha256": inventory.get("dtb_sha256"),
        "clients": rows,
        "summary": {
            "clients": len(rows),
            "complete": sum(row["status"] == "complete" for row in rows),
            "partial": sum(row["status"] != "complete" for row in rows),
            "complete_request_profiles": sum(
                row["request_profile_status"] == "complete" for row in rows
            ),
            "partial_request_profiles": sum(
                row["request_profile_status"] != "complete" for row in rows
            ),
            "opp_mismatches": sum(
                row["available_frequencies_match_opp_table"] is False
                for row in rows
            ),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, default=DEFAULT_INVENTORY)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        inventory = json.loads(args.inventory.read_text())
        result = capture(inventory)
    except (OSError, json.JSONDecodeError, RuntimeError, ValueError) as error:
        print(f"capture failed: {error}", file=sys.stderr)
        return 2
    rendered = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered)
    else:
        print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
