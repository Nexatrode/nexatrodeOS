#!/usr/bin/env python3
"""Audit a boot-time dynamic-debug capture of Sunfish MSM-bus requests.

This parses registration routes, request-index transitions, and devfreq_devbw
AB/IB logs. It does not compute a complete BCM vote or authorize a hardware
request.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
from pathlib import Path


FIND_RE = re.compile(r"register_client_adhoc:find path\.src\s+(-?\d+) dest\s+(-?\d+)")
FAIL_RE = re.compile(r"register_client_adhoc:Failed to find path\.src\s+(-?\d+) dest\s+(-?\d+)")
HANDLE_RE = re.compile(r"register_client_adhoc:Client handle\s+(\d+)\s+(\S+)")
UPDATE_RE = re.compile(
    r"update_request_adhoc: cl:\s+(\d+) index:\s+(-?\d+) "
    r"curr:\s+(-?\d+) num_paths:\s+(\d+)"
)
UNCHANGED_RE = re.compile(r"update_request_adhoc: Not updating client request idx\s+(\d+) unchanged")
DEVBW_RE = re.compile(
    r"(?:devbw\s+(?P<driver_first_device>.*?)\s*:\s+"
    r"|(?P<legacy_device>.*?)\s+devbw:\s+)"
    r"BW MBps: AB:\s+(?P<average>-?\d+) IB:\s+(?P<instantaneous>-?\d+)"
)
BUS_ERROR_MARKERS = (
    "Err updating client request",
    "Err updating path",
    "Update path failed",
    "error sending active/awake sets",
    "error sending wake sets",
    "error sending sleep sets",
)
KERNEL_TIME_RE = re.compile(r"^\[\s*(\d+(?:\.\d+)?)\]")

# Runtime clients backed by fixed C tables instead of qcom,msm-bus DT nodes.
# Values are bytes/s as assigned to struct msm_bus_vectors by the pinned kernel.
# Route matching stays exact so a different target/boot cannot inherit these
# profiles merely because its debug name happens to match.
KNOWN_RUNTIME_PROFILES = (
    {
        "name": "scm_pas",
        "path_signature": ((125, 512),),
        "active_only": False,
        "source_profile": "drivers/soc/qcom/subsys-pil-tz.c:scm_pas_bw_tbl",
        "cases": (
            ("OFF", 0, 0),
            ("PAS_LOAD", 393_600_000, 3_936_000_000),
        ),
    },
    {
        "name": "sde_reg",
        "path_signature": ((1, 590),),
        "active_only": True,
        "source_profile": "drivers/media/platform/msm/sde/rotator/sde_rotator_base.c:mdp_reg_bus_vectors",
        "cases": (
            ("DISABLE", 0, 0),
            ("19_MHZ", 0, 153_600_000),
            ("40_MHZ", 0, 320_000_000),
            ("80_MHZ", 0, 640_000_000),
        ),
    },
)


def parse_trace(text: str) -> dict:
    registrations: list[dict] = []
    by_handle: dict[int, dict] = {}
    pending_paths: list[tuple[int, int]] = []
    updates: list[dict] = []
    devbw_samples: list[dict] = []
    unchanged_notes: list[dict] = []
    failed_paths: list[dict] = []
    bus_error_lines: list[dict] = []
    anomalies: list[dict] = []
    orphan_paths: list[dict] = []

    for line_number, line in enumerate(text.splitlines(), 1):
        time_match = KERNEL_TIME_RE.match(line)
        kernel_time = float(time_match.group(1)) if time_match else None
        match = FIND_RE.search(line)
        if match:
            pending_paths.append((int(match.group(1)), int(match.group(2))))
            continue

        match = FAIL_RE.search(line)
        if match:
            src, dst = int(match.group(1)), int(match.group(2))
            failed_paths.append({"line": line_number, "source_id": src, "destination_id": dst})
            if pending_paths and pending_paths[-1] == (src, dst):
                pending_paths.pop()
            continue

        match = HANDLE_RE.search(line)
        if match:
            handle, name = int(match.group(1)), match.group(2)
            registration = {
                "handle": handle,
                "name": name,
                "paths": [
                    {"source_id": src, "destination_id": dst}
                    for src, dst in pending_paths
                ],
                "initial_index": -1,
                "final_index": -1,
                "updates": [],
            }
            if kernel_time is not None:
                registration["registered_at_seconds"] = kernel_time
            if handle in by_handle:
                anomalies.append({"line": line_number, "kind": "duplicate_handle", "handle": handle})
            else:
                by_handle[handle] = registration
            registrations.append(registration)
            pending_paths = []
            continue

        match = UPDATE_RE.search(line)
        if match:
            handle, index, prior, num_paths = (int(value) for value in match.groups())
            update = {
                "line": line_number,
                "handle": handle,
                "index": index,
                "reported_prior_index": prior,
                "num_paths": num_paths,
            }
            if kernel_time is not None:
                update["kernel_time_seconds"] = kernel_time
            registration = by_handle.get(handle)
            if registration is None:
                update["matched_registration"] = False
                anomalies.append({"line": line_number, "kind": "update_before_or_without_registration", "handle": handle})
            else:
                update["matched_registration"] = True
                expected_prior = registration["final_index"]
                update["prior_index_matches"] = prior == expected_prior
                if prior != expected_prior:
                    anomalies.append({
                        "line": line_number,
                        "kind": "prior_index_mismatch",
                        "handle": handle,
                        "expected": expected_prior,
                        "reported": prior,
                    })
                path_count = len(registration["paths"])
                update["registered_path_count"] = path_count
                update["path_count_matches"] = path_count == 0 or num_paths == path_count
                if path_count and num_paths != path_count:
                    anomalies.append({
                        "line": line_number,
                        "kind": "path_count_mismatch",
                        "handle": handle,
                        "registered": path_count,
                        "reported": num_paths,
                    })
                registration["final_index"] = index
                registration["updates"].append(update)
            updates.append(update)
            continue

        match = UNCHANGED_RE.search(line)
        if match:
            unchanged_notes.append({"line": line_number, "index": int(match.group(1))})

        match = DEVBW_RE.search(line)
        if match:
            device_name = (
                match.group("driver_first_device")
                or match.group("legacy_device")
            ).rsplit(" : ", 1)[-1].strip()
            devbw_samples.append({
                "line": line_number,
                "device_name": device_name,
                "average_mb_s": int(match.group("average")),
                "instantaneous_mb_s": int(match.group("instantaneous")),
                "raw_line": line.strip(),
                **({"kernel_time_seconds": kernel_time} if kernel_time is not None else {}),
            })

        if any(marker in line for marker in BUS_ERROR_MARKERS):
            bus_error_lines.append({"line": line_number, "raw_line": line.strip()})

    orphan_paths = [
        {"source_id": src, "destination_id": dst}
        for src, dst in pending_paths
    ]
    if orphan_paths:
        anomalies.append({"kind": "orphan_registration_paths", "count": len(orphan_paths)})

    return {
        "scope": (
            "Boot-time client index transitions and devfreq_devbw request values. "
            "Not a complete client/vote snapshot and not hardware-write authority."
        ),
        "summary": {
            "registered_handles": len(registrations),
            "request_updates": len(updates),
            "matched_updates": sum(bool(row.get("matched_registration")) for row in updates),
            "devbw_samples": len(devbw_samples),
            "unchanged_index_notes_without_handle": len(unchanged_notes),
            "failed_path_logs": len(failed_paths),
            "orphan_registration_paths": len(orphan_paths),
            "bus_error_lines": len(bus_error_lines),
            "anomalies": len(anomalies),
        },
        "registrations": registrations,
        "request_updates": updates,
        "devbw_samples": devbw_samples,
        "unchanged_index_notes_without_handle": unchanged_notes,
        "failed_paths": failed_paths,
        "orphan_paths": orphan_paths,
        "bus_error_lines": bus_error_lines,
        "anomalies": anomalies,
    }


def parse_time_windows(values: list[str]) -> list[dict]:
    """Parse NAME:START:END monotonic-uptime windows, with exclusive ends."""
    windows = []
    names = set()
    for value in values:
        parts = value.split(":")
        if len(parts) != 3 or not parts[0]:
            raise ValueError(f"invalid time window {value!r}; expected NAME:START:END")
        name = parts[0]
        try:
            start, end = float(parts[1]), float(parts[2])
        except ValueError as error:
            raise ValueError(f"invalid time window bounds in {value!r}") from error
        if not (math.isfinite(start) and math.isfinite(end) and start >= 0 and end > start):
            raise ValueError(f"invalid time window range in {value!r}")
        if name in names:
            raise ValueError(f"duplicate time window name {name!r}")
        names.add(name)
        windows.append({"name": name, "start_seconds": start, "end_seconds": end})
    return windows


def summarize_time_windows(trace: dict, windows: list[dict]) -> list[dict]:
    """Summarize request transitions and devbw samples within boot-time windows."""
    registration_names = {
        item.get("handle"): item.get("name", "unknown")
        for item in trace.get("registrations", [])
    }
    summaries = []
    for window in windows:
        start, end = window["start_seconds"], window["end_seconds"]
        updates_by_client: dict[str, dict] = {}
        untimed_updates = 0
        for update in trace.get("request_updates", []):
            event_time = update.get("kernel_time_seconds")
            if not isinstance(event_time, (int, float)):
                untimed_updates += 1
                continue
            if not start <= event_time < end:
                continue
            handle = update.get("handle")
            name = registration_names.get(handle, "unregistered")
            key = f"{name}#{handle}"
            row = updates_by_client.setdefault(key, {
                "name": name,
                "handle": handle,
                "updates": 0,
                "transitions": {},
                "first_seconds": event_time,
                "last_seconds": event_time,
            })
            transition = f"{update.get('reported_prior_index')}->{update.get('index')}"
            row["updates"] += 1
            row["transitions"][transition] = row["transitions"].get(transition, 0) + 1
            row["first_seconds"] = min(row["first_seconds"], event_time)
            row["last_seconds"] = max(row["last_seconds"], event_time)

        devbw_by_device: dict[str, dict] = {}
        untimed_devbw = 0
        for sample in trace.get("devbw_samples", []):
            event_time = sample.get("kernel_time_seconds")
            if not isinstance(event_time, (int, float)):
                untimed_devbw += 1
                continue
            if not start <= event_time < end:
                continue
            device = sample.get("device_name", "unknown")
            average = sample["average_mb_s"]
            instantaneous = sample["instantaneous_mb_s"]
            row = devbw_by_device.setdefault(device, {
                "samples": 0,
                "average_mb_s_min": average,
                "average_mb_s_max": average,
                "instantaneous_mb_s_min": instantaneous,
                "instantaneous_mb_s_max": instantaneous,
                "first_seconds": event_time,
                "last_seconds": event_time,
            })
            row["samples"] += 1
            row["average_mb_s_min"] = min(row["average_mb_s_min"], average)
            row["average_mb_s_max"] = max(row["average_mb_s_max"], average)
            row["instantaneous_mb_s_min"] = min(row["instantaneous_mb_s_min"], instantaneous)
            row["instantaneous_mb_s_max"] = max(row["instantaneous_mb_s_max"], instantaneous)
            row["first_seconds"] = min(row["first_seconds"], event_time)
            row["last_seconds"] = max(row["last_seconds"], event_time)

        summaries.append({
            **window,
            "interval": "start-inclusive, end-exclusive kernel uptime seconds",
            "request_update_count": sum(row["updates"] for row in updates_by_client.values()),
            "request_updates_without_kernel_timestamp": untimed_updates,
            "updates_by_client": updates_by_client,
            "devbw_sample_count": sum(row["samples"] for row in devbw_by_device.values()),
            "devbw_samples_without_kernel_timestamp": untimed_devbw,
            "devbw_by_device": devbw_by_device,
        })
    return summaries


def join_static_inventory(trace: dict, inventory: dict) -> dict:
    """Join final per-handle usecases to DT or exact source-backed vectors."""

    joined = []
    fabric = inventory.get("fabric_topology", {})
    fabric_nodes = {
        row.get("node"): row for row in fabric.get("bus_nodes", [])
    }
    for registration in trace.get("registrations", []):
        runtime_signature = tuple(
            (path.get("source_id"), path.get("destination_id"))
            for path in registration.get("paths", [])
        )
        candidates = []
        for client in inventory.get("clients", []):
            cases = client.get("cases", [])
            if client.get("name") != registration.get("name") or not cases:
                continue
            first_paths = cases[0].get("paths", [])
            static_signature = tuple(
                (path.get("source_id"), path.get("destination_id"))
                for path in first_paths
            )
            if static_signature == runtime_signature:
                candidates.append(client)

        profile_name = None
        for profile in KNOWN_RUNTIME_PROFILES:
            if (
                registration.get("name") == profile["name"]
                and runtime_signature == profile["path_signature"]
            ):
                profile_name = profile["source_profile"]
                candidates.append({
                    "name": profile["name"],
                    "node": profile["source_profile"],
                    "vector_property": "bytes_per_second",
                    "active_only": profile["active_only"],
                    "source_profile": profile["source_profile"],
                    "cases": [
                        {
                            "name": case_name,
                            "paths": [{
                                "path_index": 0,
                                "source_id": runtime_signature[0][0],
                                "destination_id": runtime_signature[0][1],
                                "average": average,
                                "instantaneous": instantaneous,
                            }],
                        }
                        for case_name, average, instantaneous in profile["cases"]
                    ],
                })
                break

        row = {
            "handle": registration.get("handle"),
            "name": registration.get("name"),
            "final_index": registration.get("final_index", -1),
            "candidate_nodes": [client.get("node") for client in candidates],
            "status": "no_static_match",
        }
        if not candidates:
            joined.append(row)
            continue

        case_signatures = []
        route_mismatch = False
        for client in candidates:
            cases = client.get("cases", [])
            signatures = [
                tuple(
                    (path.get("source_id"), path.get("destination_id"))
                    for path in case.get("paths", [])
                )
                for case in cases
            ]
            if any(signature != runtime_signature for signature in signatures):
                route_mismatch = True
            index = row["final_index"]
            if index >= 0 and index < len(cases):
                selected = cases[index]
                case_signatures.append(json.dumps({
                    "vector_property": client.get("vector_property"),
                    "active_only": client.get("active_only"),
                    "paths": selected.get("paths", []),
                }, sort_keys=True))

        if route_mismatch:
            row["status"] = "static_case_routes_vary"
            joined.append(row)
            continue
        if row["final_index"] == -1:
            row["status"] = "inactive"
            joined.append(row)
            continue
        if any(row["final_index"] >= len(client.get("cases", [])) for client in candidates):
            row["status"] = "usecase_index_out_of_range"
            joined.append(row)
            continue
        if len(set(case_signatures)) != 1:
            row["status"] = "ambiguous_static_match"
            joined.append(row)
            continue

        client = candidates[0]
        case = client["cases"][row["final_index"]]
        vector_property = client.get("vector_property")
        active_only = bool(client.get("active_only"))
        path_votes = []
        for path in case.get("paths", []):
            average_raw = int(path["average"])
            instantaneous_raw = int(path["instantaneous"])
            if vector_property == "qcom,msm-bus,vectors-KBps":
                average_bps = average_raw * 1000
                instantaneous_bps = instantaneous_raw * 1000
                unit_status = "bytes_per_second"
            elif vector_property == "bytes_per_second":
                average_bps = average_raw
                instantaneous_bps = instantaneous_raw
                unit_status = "bytes_per_second_from_pinned_kernel_table"
            else:
                average_bps = None
                instantaneous_bps = None
                unit_status = "unconverted_vector_property"
            vector = {
                "average_raw": average_raw,
                "instantaneous_raw": instantaneous_raw,
                "average_bytes_per_second": average_bps,
                "instantaneous_bytes_per_second": instantaneous_bps,
                "unit_status": unit_status,
            }
            zero = {
                "average_bytes_per_second": 0,
                "instantaneous_bytes_per_second": 0,
            }
            if active_only:
                active, dual = vector, zero
            else:
                active, dual = zero, vector
            path_votes.append({
                **{key: path.get(key) for key in (
                    "path_index", "source_id", "destination_id", "route", "bcms"
                )},
                "active": active,
                "dual": dual,
            })
            if fabric_nodes and path.get("route"):
                path_votes[-1]["bcm_bindings"] = route_bcm_bindings(
                    path.get("route", []), fabric_nodes
                )

        row.update({
            "status": (
                "matched_source_profile" if profile_name else
                "matched_static" if len(candidates) == 1 else
                "equivalent_static_matches"
            ),
            "case_name": case.get("name"),
            "vector_property": vector_property,
            "active_only": active_only,
            "paths": path_votes,
        })
        if profile_name:
            row["source_profile"] = profile_name
        joined.append(row)

    counts: dict[str, int] = {}
    for row in joined:
        counts[row["status"]] = counts.get(row["status"], 0) + 1
    return {
        "summary": counts,
        "clients": joined,
        "devbw_request_samples": join_devbw_samples(trace, inventory),
    }


def join_static_request_transitions(trace: dict, inventory: dict) -> dict:
    """Resolve each logged old/new index to its exact static vector when known."""
    resolved_statuses = {
        "matched_static", "equivalent_static_matches", "matched_source_profile", "inactive"
    }
    registrations = {}
    for registration in trace.get("registrations", []):
        handle = registration.get("handle")
        if handle in registrations:
            raise ValueError(f"duplicate request-trace handle {handle}")
        registrations[handle] = registration

    def state_for(registration: dict, index: int) -> dict:
        state_registration = {**registration, "final_index": index}
        joined = join_static_inventory(
            {"registrations": [state_registration], "devbw_samples": []}, inventory
        )
        return joined["clients"][0]

    rows = []
    for update in trace.get("request_updates", []):
        handle = update.get("handle")
        registration = registrations.get(handle)
        row = {
            "line": update.get("line"),
            "handle": handle,
            "reported_prior_index": update.get("reported_prior_index"),
            "index": update.get("index"),
            "num_paths": update.get("num_paths"),
        }
        if registration is None or not update.get("matched_registration"):
            row["status"] = "registration_missing"
            rows.append(row)
            continue

        row["name"] = registration.get("name")
        prior_index = update.get("reported_prior_index")
        index = update.get("index")
        if not isinstance(prior_index, int) or not isinstance(index, int):
            row["status"] = "invalid_index"
            rows.append(row)
            continue

        new_state = state_for(registration, index)
        row["new_state"] = new_state
        if prior_index >= -1:
            row["old_state"] = state_for(registration, prior_index)

        if update.get("path_count_matches") is False:
            row["status"] = "path_count_mismatch"
        elif update.get("prior_index_matches") is False:
            row["status"] = "reported_prior_index_mismatch"
        elif new_state.get("status") in resolved_statuses:
            old_state = row.get("old_state", {})
            row["status"] = (
                "known_transition"
                if old_state.get("status") in resolved_statuses
                else "old_state_unresolved"
            )
        else:
            row["status"] = new_state.get("status", "unresolved_transition")
        rows.append(row)

    counts: dict[str, int] = {}
    for row in rows:
        counts[row["status"]] = counts.get(row["status"], 0) + 1
    return {"summary": counts, "updates": rows}


def route_bcm_bindings(route: list[str], fabric_nodes: dict[str, dict]) -> list[dict]:
    """Attach physical BCM device and RSC identities in route traversal order."""
    result = []
    for route_index, node_name in enumerate(route):
        bus_node = fabric_nodes.get(node_name)
        if bus_node is None:
            continue
        for binding in bus_node.get("bcm_bindings", []):
            result.append({
                "route_index": route_index,
                "bus_node": node_name,
                "bus_node_id": bus_node.get("cell_id"),
                "binding_index": binding.get("binding_index"),
                "bcm_node": binding.get("node"),
                "bcm_node_id": binding.get("cell_id"),
                "cmd_db_name": binding.get("cmd_db_name"),
                "rsc_node": (binding.get("commit_rsc") or {}).get("node"),
                "rsc_id": (binding.get("commit_rsc") or {}).get("cell_id"),
            })
    return result


def join_devbw_samples(trace: dict, inventory: dict) -> dict:
    """Resolve dynamic devfreq samples against exact DT routes and updates.

    The pinned devfreq_devbw driver logs AB/IB in MiB/s, divides AB evenly
    across the registered paths with integer truncation, and repeats IB on
    every path. Its two usecase indices are buffer slots, not OPP identifiers.
    """
    clients_by_name: dict[str, list[dict]] = {}
    for client in inventory.get("devbw_clients", []):
        runtime_name = client.get("runtime_name")
        if runtime_name:
            clients_by_name.setdefault(runtime_name, []).append(client)
    fabric_nodes = {
        row.get("node"): row
        for row in inventory.get("fabric_topology", {}).get("bus_nodes", [])
    }
    registrations_by_name: dict[str, list[dict]] = {}
    for registration in trace.get("registrations", []):
        registrations_by_name.setdefault(registration.get("name", ""), []).append(registration)
    updates_by_handle: dict[int, list[dict]] = {}
    for update in trace.get("request_updates", []):
        updates_by_handle.setdefault(update.get("handle"), []).append(update)
    for updates in updates_by_handle.values():
        updates.sort(key=lambda row: row["line"])

    requests = []
    used_update_lines: set[int] = set()
    for sample in trace.get("devbw_samples", []):
        name = sample.get("device_name", "")
        row = {
            "sample_line": sample.get("line"),
            "device_name": name,
            "average_mb_s": sample.get("average_mb_s"),
            "instantaneous_mb_s": sample.get("instantaneous_mb_s"),
            "status": "no_devbw_inventory_match",
        }
        dt_candidates = clients_by_name.get(name, [])
        if len(dt_candidates) != 1:
            row["candidate_nodes"] = [client.get("node") for client in dt_candidates]
            if len(dt_candidates) > 1:
                row["status"] = "ambiguous_devbw_inventory_match"
            requests.append(row)
            continue

        registrations = registrations_by_name.get(name, [])
        if len(registrations) != 1:
            row["candidate_handles"] = [item.get("handle") for item in registrations]
            if len(registrations) > 1:
                row["status"] = "ambiguous_runtime_registration"
            else:
                row["status"] = "no_runtime_registration"
            requests.append(row)
            continue

        client = dt_candidates[0]
        registration = registrations[0]
        runtime_signature = tuple(
            (path.get("source_id"), path.get("destination_id"))
            for path in registration.get("paths", [])
        )
        dt_signature = tuple(
            (path.get("source_id"), path.get("destination_id"))
            for path in client.get("paths", [])
        )
        row["handle"] = registration.get("handle")
        row["node"] = client.get("node")
        if runtime_signature != dt_signature:
            row["status"] = "runtime_route_mismatch"
            row["runtime_path_signature"] = runtime_signature
            row["dt_path_signature"] = dt_signature
            requests.append(row)
            continue
        if any("route_error" in path for path in client.get("paths", [])):
            row["status"] = "unresolved_dt_route"
            requests.append(row)
            continue

        following_updates = [
            update for update in updates_by_handle.get(registration["handle"], [])
            if update["line"] > sample["line"] and update["line"] not in used_update_lines
        ]
        if not following_updates:
            row["status"] = "no_following_request_update"
            requests.append(row)
            continue
        update = following_updates[0]
        used_update_lines.add(update["line"])
        row["request_update_line"] = update["line"]
        row["buffer_index"] = update.get("index")
        row["active_only"] = bool(client.get("active_only"))
        path_count = len(client.get("paths", []))
        if update.get("num_paths") != path_count:
            row["status"] = "request_path_count_mismatch"
            row["reported_path_count"] = update.get("num_paths")
            row["dt_path_count"] = path_count
            requests.append(row)
            continue
        if path_count == 0:
            row["status"] = "empty_devbw_path_set"
            requests.append(row)
            continue

        average = int(sample["average_mb_s"])
        instantaneous = int(sample["instantaneous_mb_s"])
        if average < 0 or instantaneous < 0:
            row["status"] = "negative_devbw_value"
            requests.append(row)
            continue
        average_per_path = (average // path_count) * (1 << 20)
        instantaneous_per_path = instantaneous * (1 << 20)
        zero = {
            "average_bytes_per_second": 0,
            "instantaneous_bytes_per_second": 0,
        }
        request = {
            "average_mb_s_total": average,
            "instantaneous_mb_s": instantaneous,
            "average_bytes_per_second_per_path": average_per_path,
            "instantaneous_bytes_per_second_per_path": instantaneous_per_path,
            "byte_unit": "MiB/s converted with MBYTE = 1 << 20",
        }
        if client.get("active_only"):
            active = request
            dual = zero
        else:
            active = zero
            dual = request
        row["paths"] = [
            {
                **path,
                "active": active,
                "dual": dual,
                **({"bcm_bindings": route_bcm_bindings(path.get("route", []), fabric_nodes)}
                   if fabric_nodes and path.get("route") else {}),
            }
            for path in client["paths"]
        ]
        row["status"] = "matched_devbw_request_update"
        requests.append(row)

    counts: dict[str, int] = {}
    for row in requests:
        counts[row["status"]] = counts.get(row["status"], 0) + 1
    return {"summary": counts, "samples": requests}


def audit(log_path: Path) -> dict:
    raw = log_path.read_bytes()
    result = parse_trace(raw.decode(errors="replace"))
    result["input_sha256"] = hashlib.sha256(raw).hexdigest()
    result["input_bytes"] = len(raw)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path, help="captured kernel log from one boot")
    parser.add_argument("--inventory", type=Path, help="optional DT/runtime route inventory JSON")
    parser.add_argument(
        "--window",
        action="append",
        default=[],
        metavar="NAME:START:END",
        help="summarize events in a half-open kernel-uptime window; may be repeated",
    )
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = audit(args.log)
    try:
        windows = parse_time_windows(args.window)
    except ValueError as error:
        parser.error(str(error))
    if windows:
        result["time_window_summaries"] = summarize_time_windows(result, windows)
    if args.inventory:
        inventory = json.loads(args.inventory.read_text())
        result["static_request_join"] = join_static_inventory(result, inventory)
        result["static_transition_join"] = join_static_request_transitions(
            result, inventory
        )
        result["devbw_request_join"] = join_devbw_samples(result, inventory)
    rendered = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered)
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
