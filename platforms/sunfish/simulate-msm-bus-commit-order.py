#!/usr/bin/env python3
"""Reconstruct touched bus-node and BCM order from a Sunfish update trace.

This is a source-derived ordering report only. It does not compute BCM vote
words, claim a complete live vote set, or submit a bus/RPMh request. Optional
observed CMD_DB metadata can annotate BCMs and produce a candidate VCD grouping;
that grouping is not a reconstruction of the actual TCS command list.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def load_json(path: Path) -> tuple[dict, bytes]:
    raw = path.read_bytes()
    return json.loads(raw), raw


def registration_paths(registration: dict) -> tuple[tuple[int, int], ...]:
    return tuple(
        (int(path["source_id"]), int(path["destination_id"]))
        for path in registration.get("paths", [])
    )


def simulate(trace: dict, inventory: dict, bcm_metadata: dict | None = None) -> dict:
    """Reconstruct the per-update ordered touch lists when routes are verified."""
    snapshot = inventory.get("runtime_registration_snapshot")
    if not isinstance(snapshot, dict):
        raise ValueError("inventory has no runtime registration snapshot")
    if snapshot.get("summary", {}).get("unresolved_paths", 0):
        raise ValueError("runtime registration snapshot contains unresolved routes")
    fabric = inventory.get("fabric_topology")
    if not isinstance(fabric, dict):
        raise ValueError("inventory has no fabric topology")

    metadata_resources = {}
    metadata_source = None
    if bcm_metadata is not None:
        if bcm_metadata.get("target") != "sunfish":
            raise ValueError("BCM metadata target must be sunfish")
        metadata_resources = bcm_metadata.get("resources")
        if not isinstance(metadata_resources, dict):
            raise ValueError("BCM metadata has no resources mapping")
        metadata_source = bcm_metadata.get("metadata_source")
        for name, row in metadata_resources.items():
            if not isinstance(name, str) or not isinstance(row, dict):
                raise ValueError("invalid BCM metadata resource entry")
            domain = row.get("clock_domain")
            if type(domain) is not int or not 0 <= domain < 16:
                raise ValueError(f"{name}: invalid observed CMD_DB clock_domain")

    bus_nodes = {}
    for row in fabric.get("bus_nodes", []):
        name = row.get("node")
        if not isinstance(name, str) or name in bus_nodes:
            raise ValueError(f"invalid or duplicate fabric bus-node identity: {name!r}")
        bus_nodes[name] = row

    snapshot_by_identity = {}
    snapshot_rows = []
    for row in snapshot.get("registrations", []):
        handle = int(row["handle"])
        if any(int(existing["handle"]) == handle for existing in snapshot_rows):
            raise ValueError(f"duplicate baseline registration handle {handle}")
        snapshot_rows.append(row)
        identity = (row.get("name"), registration_paths(row))
        snapshot_by_identity.setdefault(identity, []).append(row)

    trace_by_handle = {}
    for row in trace.get("registrations", []):
        handle = int(row["handle"])
        if handle in trace_by_handle:
            raise ValueError(f"duplicate trace registration handle {handle}")
        trace_by_handle[handle] = row

    verified_routes = {}
    registration_issues = []
    matched_baseline_handles = set()
    for handle, observed in trace_by_handle.items():
        identity = (observed.get("name"), registration_paths(observed))
        candidates = snapshot_by_identity.get(identity, [])
        baseline = next(
            (row for row in candidates if int(row["handle"]) not in matched_baseline_handles),
            None,
        )
        if baseline is None:
            same_name = [row for row in snapshot_rows if row.get("name") == observed.get("name")]
            if same_name:
                registration_issues.append({
                    "handle": handle,
                    "issue": "path_signature_differs_from_baseline",
                    "name": observed.get("name"),
                    "trace_paths": identity[1],
                    "baseline_path_signatures": [registration_paths(row) for row in same_name],
                })
            else:
                registration_issues.append({
                    "handle": handle,
                    "issue": "no_baseline_registration",
                    "name": observed.get("name"),
                })
            continue
        matched_baseline_handles.add(int(baseline["handle"]))
        routes = []
        valid = True
        for path in baseline.get("paths", []):
            route = path.get("route")
            if path.get("route_error") or not isinstance(route, list) or not route:
                registration_issues.append({
                    "handle": handle,
                    "issue": "missing_or_unresolved_route",
                    "path": [path.get("source_id"), path.get("destination_id")],
                })
                valid = False
                break
            unknown = [node for node in route if node not in bus_nodes]
            if unknown:
                registration_issues.append({
                    "handle": handle,
                    "issue": "route_node_missing_from_fabric",
                    "nodes": unknown,
                })
                valid = False
                break
            routes.append(route)
        if valid:
            verified_routes[handle] = routes

    for baseline in snapshot_rows:
        handle = int(baseline["handle"])
        if handle not in matched_baseline_handles:
            registration_issues.append({
                "handle": handle,
                "issue": "baseline_registration_missing_from_trace",
                "name": baseline.get("name"),
            })

    events = []
    event_issues = []
    for update in trace.get("request_updates", []):
        handle = int(update["handle"])
        registration = trace_by_handle.get(handle)
        if not update.get("matched_registration") or registration is None:
            event_issues.append({
                "line": update.get("line"),
                "handle": handle,
                "issue": "update_without_registration",
            })
            continue
        if handle not in verified_routes:
            event_issues.append({
                "line": update.get("line"),
                "handle": handle,
                "issue": "registration_route_not_verified",
            })
            continue
        if update.get("path_count_matches") is False:
            event_issues.append({
                "line": update.get("line"),
                "handle": handle,
                "issue": "reported_path_count_mismatch",
            })
            continue
        if update.get("prior_index_matches") is False:
            event_issues.append({
                "line": update.get("line"),
                "handle": handle,
                "issue": "reported_prior_index_mismatch",
            })
            continue

        commit_nodes = []
        commit_node_set = set()

        def append_node(node: str) -> None:
            if node not in bus_nodes:
                raise ValueError(f"commit-list node {node!r} is missing from fabric topology")
            if node not in commit_node_set:
                commit_node_set.add(node)
                commit_nodes.append(node)

        route_rows = []
        routes = verified_routes[handle]
        if len(routes) != len(registration.get("paths", [])):
            event_issues.append({
                "line": update.get("line"),
                "handle": handle,
                "issue": "verified_route_count_mismatch",
            })
            continue
        for path_index, (path, route) in enumerate(zip(registration["paths"], routes)):
            for node_name in route:
                # update_path() visits each route node. add_node_to_clist() adds
                # that node followed by its bus_device parent, both once per
                # commit transaction.
                append_node(node_name)
                parent = bus_nodes[node_name].get("bus_device")
                if not isinstance(parent, dict) or not isinstance(parent.get("node"), str):
                    raise ValueError(
                        f"route node {node_name!r} has no resolved qcom,bus-dev parent"
                    )
                append_node(parent["node"])
            route_rows.append({
                "path_index": path_index,
                "source_id": path.get("source_id"),
                "destination_id": path.get("destination_id"),
                "route_nodes": route,
            })

        bcm_order = []
        bcm_seen = set()
        rsc_order = []
        for bus_node in commit_nodes:
            row = bus_nodes[bus_node]
            for binding in row.get("bcm_bindings", []):
                identity = binding.get("phandle")
                if identity is None:
                    raise ValueError(f"{bus_node}: BCM binding has no phandle identity")
                if identity in bcm_seen:
                    continue
                bcm_seen.add(identity)
                rsc = binding.get("commit_rsc")
                if not isinstance(rsc, dict) or not isinstance(rsc.get("node"), str):
                    raise ValueError(
                        f"{binding.get('node')!r}: BCM has no resolved qcom,rscs[0]"
                    )
                if rsc["node"] not in rsc_order:
                    rsc_order.append(rsc["node"])
                cmd_db_name = binding.get("cmd_db_name")
                cmd_db_row = metadata_resources.get(cmd_db_name)
                bcm_order.append({
                    "first_touch_bus_node": bus_node,
                    "binding_index": binding.get("binding_index"),
                    "bcm_phandle": identity,
                    "bcm_node": binding.get("node"),
                    "bcm_cell_id": binding.get("cell_id"),
                    "cmd_db_name": cmd_db_name,
                    "rsc_node": rsc.get("node"),
                    "rsc_cell_id": rsc.get("cell_id"),
                    "vcd_index": None,
                    "observed_cmd_db_metadata": cmd_db_row,
                    "observed_clock_domain": (
                        cmd_db_row.get("clock_domain") if cmd_db_row else None
                    ),
                })

        if len(rsc_order) > 1:
            event_issues.append({
                "line": update.get("line"),
                "handle": handle,
                "issue": "multiple_rscs_in_one_commit_transaction",
                "rsc_nodes": rsc_order,
            })
            continue

        unknown_metadata = [
            {
                "bcm_node": row["bcm_node"],
                "bcm_phandle": row["bcm_phandle"],
                "cmd_db_name": row["cmd_db_name"],
            }
            for row in bcm_order
            if row["observed_clock_domain"] is None
        ]
        vcd_groups = {}
        if not unknown_metadata:
            # The pinned driver groups BCMs by VCD in ascending order while
            # preserving the BCM-list order within each VCD. Keep this labeled
            # as a candidate: vote aggregation and TCS filtering are not modeled.
            for row in bcm_order:
                vcd_groups.setdefault(row["observed_clock_domain"], []).append(
                    {
                        "bcm_node": row["bcm_node"],
                        "bcm_phandle": row["bcm_phandle"],
                        "cmd_db_name": row["cmd_db_name"],
                    }
                )
        events.append({
            "line": update.get("line"),
            "handle": handle,
            "client_name": registration.get("name"),
            "from_index": update.get("reported_prior_index"),
            "to_index": update.get("index"),
            "num_paths": update.get("num_paths"),
            "paths": route_rows,
            "commit_bus_nodes": commit_nodes,
            "rsc_first_touch_order": rsc_order,
            "bcm_first_touch_order": bcm_order,
            "vcd_grouped_bcm_touch_candidate": (
                [
                    {"vcd_index": vcd, "bcms": rows}
                    for vcd, rows in sorted(vcd_groups.items())
                ]
                if not unknown_metadata else None
            ),
            "vcd_candidate_unresolved_bcms": unknown_metadata,
            "kernel_bus_errors_present": bool(trace.get("bus_error_lines")),
            "status": (
                "ordered_touches_reconstructed_with_kernel_errors"
                if trace.get("bus_error_lines") else
                "ordered_bus_and_bcm_touches_reconstructed"
            ),
        })

    return {
        "scope": (
            "Expected per-update bus-node and BCM first-touch order reconstructed "
            "from the pinned Linux MSM-bus path walk and captured client update "
            "sequence. This is not proof of a successful hardware transaction, "
            "a complete live vote snapshot, or a numeric BCM/TCS plan."
        ),
        "source_order_basis": {
            "path_vector_order": "msm_bus_arb_adhoc.c:update_client_paths iterates vectors[i] in increasing i",
            "route_and_parent_order": "msm_bus_arb_adhoc.c:update_path plus add_node_to_clist add route node then bus parent once",
            "bcm_binding_order": "msm_bus_fabric_rpmh.c:bcm_clist_add iterates node_info->bcm_devs[i]",
            "rsc_selection": "msm_bus_fabric_rpmh.c:bcm_clist_add uses qcom,rscs[0]",
            "vcd_group_order": "msm_bus_fabric_rpmh.c:tcs_cmd_list_gen groups by ascending VCD; observed CMD_DB clock_domain metadata enables a BCM touch-order candidate, not actual TCS command reconstruction",
        },
        "inputs": {
            "trace_input_sha256": trace.get("input_sha256"),
            "kernel_commit": inventory.get("kernel_commit"),
            "dtb_sha256": inventory.get("dtb_sha256"),
            "baseline_registration_log_sha256": snapshot.get("dmesg_sha256"),
            "bcm_metadata_source": metadata_source,
        },
        "summary": {
            "trace_request_updates": len(trace.get("request_updates", [])),
            "simulated_updates": len(events),
            "registration_issues": len(registration_issues),
            "event_issues": len(event_issues),
            "trace_bus_errors": len(trace.get("bus_error_lines", [])),
            "trace_anomalies": len(trace.get("anomalies", [])),
            "events_with_complete_vcd_metadata": sum(
                not event["vcd_candidate_unresolved_bcms"] for event in events
            ),
            "tcs_vcd_order_resolved": False,
        },
        "limitations": [
            "Only captured changed-index events are represented; clients without an update event may still have votes from another path or prior state.",
            "Optional CMD_DB metadata is a separate observed snapshot, not captured in this route inventory. Unknown names leave VCD grouping unresolved.",
            "VCD-grouped BCM touch candidates do not calculate vote values, filter unchanged/zero commands, or reproduce actual TCS command words/order.",
            "Bus aggregation, RPMh ownership, handoff state, and hardware writes are outside this report.",
        ],
        "registration_issues": registration_issues,
        "event_issues": event_issues,
        "events": events,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path, help="JSON report from audit-msm-bus-request-trace.py")
    parser.add_argument("--inventory", required=True, type=Path)
    parser.add_argument("--metadata", type=Path,
                        help="optional observed Sunfish CMD_DB BCM metadata JSON")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    trace, trace_raw = load_json(args.trace)
    inventory, _inventory_raw = load_json(args.inventory)
    metadata = load_json(args.metadata)[0] if args.metadata else None
    trace["input_sha256"] = trace.get("input_sha256") or hashlib.sha256(trace_raw).hexdigest()
    result = simulate(trace, inventory, metadata)
    rendered = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered)
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
