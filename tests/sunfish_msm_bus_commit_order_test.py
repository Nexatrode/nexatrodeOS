#!/usr/bin/env python3
"""Checks the source-derived Sunfish commit-list ordering simulator."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "platforms" / "sunfish" / "simulate-msm-bus-commit-order.py"
SPEC = importlib.util.spec_from_file_location("simulate_msm_bus_commit_order", MODULE_PATH)
assert SPEC and SPEC.loader
SIMULATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SIMULATOR)


def fixture() -> tuple[dict, dict]:
    route_paths = [
        {"source_id": 1, "destination_id": 3, "route": ["a", "b"]},
        {"source_id": 2, "destination_id": 4, "route": ["b", "c"]},
    ]
    trace = {
        "input_sha256": "fixture-trace",
        "registrations": [{
            "handle": 7,
            "name": "ipa",
            "paths": [{key: value for key, value in path.items() if key != "route"}
                      for path in route_paths],
        }],
        "request_updates": [{
            "line": 9,
            "handle": 7,
            "index": 1,
            "reported_prior_index": -1,
            "num_paths": 2,
            "matched_registration": True,
            "path_count_matches": True,
            "prior_index_matches": True,
        }],
        "bus_error_lines": [],
        "anomalies": [],
    }

    def bcm(node: str, phandle: int, rsc: str = "rsc-apps") -> dict:
        return {
            "binding_index": 0,
            "phandle": phandle,
            "node": node,
            "cell_id": phandle + 1000,
            "cmd_db_name": node.upper(),
            "commit_rsc": {"node": rsc, "cell_id": 8000},
        }

    inventory = {
        "kernel_commit": "fixture-kernel",
        "dtb_sha256": "fixture-dtb",
        "runtime_registration_snapshot": {
            "dmesg_sha256": "fixture-baseline",
            "summary": {"unresolved_paths": 0},
            "registrations": [{
                "handle": 7,
                "name": "ipa",
                "paths": route_paths,
            }],
        },
        "fabric_topology": {"bus_nodes": [
            {"node": "a", "bus_device": {"node": "p"}, "bcm_bindings": [
                bcm("bcm-a", 10), bcm("bcm-b", 11),
            ]},
            {"node": "b", "bus_device": {"node": "p"}, "bcm_bindings": [
                bcm("bcm-b", 11), bcm("bcm-c", 12),
            ]},
            {"node": "c", "bus_device": {"node": "q"}, "bcm_bindings": [
                bcm("bcm-c", 12),
            ]},
            {"node": "p", "bus_device": {"node": "root"}, "bcm_bindings": [
                bcm("bcm-b", 11), bcm("bcm-c", 12),
            ]},
            {"node": "q", "bus_device": {"node": "root"}, "bcm_bindings": []},
            {"node": "root", "bus_device": {"node": "top"}, "bcm_bindings": []},
            {"node": "top", "bus_device": {"node": "soc"}, "bcm_bindings": []},
            {"node": "soc", "bus_device": {"node": "", "path": "/"}, "bcm_bindings": []},
        ]},
    }
    # The route list must match the parsed registration exactly, while paths
    # in the baseline snapshot carry the pinned route traversal.
    return trace, inventory


class MsmBusCommitOrderTests(unittest.TestCase):
    def test_preserves_route_parent_and_qcom_bcm_first_touch_order(self) -> None:
        trace, inventory = fixture()
        result = SIMULATOR.simulate(trace, inventory)
        self.assertEqual(result["summary"]["simulated_updates"], 1)
        event = result["events"][0]
        self.assertEqual(event["commit_bus_nodes"], ["a", "p", "b", "c", "q"])
        self.assertEqual(
            [row["bcm_node"] for row in event["bcm_first_touch_order"]],
            ["bcm-a", "bcm-b", "bcm-c"],
        )
        self.assertEqual(event["rsc_first_touch_order"], ["rsc-apps"])
        self.assertTrue(all(row["vcd_index"] is None for row in event["bcm_first_touch_order"]))
        self.assertFalse(result["summary"]["tcs_vcd_order_resolved"])

    def test_refuses_a_path_signature_that_differs_from_baseline(self) -> None:
        trace, inventory = fixture()
        trace["registrations"][0]["paths"][0]["destination_id"] = 99
        result = SIMULATOR.simulate(trace, inventory)
        self.assertEqual(result["summary"]["simulated_updates"], 0)
        self.assertEqual(result["registration_issues"][0]["issue"],
                         "path_signature_differs_from_baseline")
        self.assertEqual(result["event_issues"][0]["issue"],
                         "registration_route_not_verified")

    def test_reports_a_baseline_client_missing_from_the_trace(self) -> None:
        trace, inventory = fixture()
        trace["registrations"] = []
        trace["request_updates"] = []
        result = SIMULATOR.simulate(trace, inventory)
        self.assertEqual(result["registration_issues"], [{
            "handle": 7,
            "issue": "baseline_registration_missing_from_trace",
            "name": "ipa",
        }])

    def test_matches_registration_by_name_and_path_when_runtime_handle_changes(self) -> None:
        trace, inventory = fixture()
        trace["registrations"][0]["handle"] = 70
        trace["request_updates"][0]["handle"] = 70
        result = SIMULATOR.simulate(trace, inventory)
        self.assertEqual(result["summary"]["simulated_updates"], 1)
        self.assertEqual(result["registration_issues"], [])
        self.assertEqual(result["event_issues"], [])

    def test_requires_resolved_parent_for_each_route_node(self) -> None:
        trace, inventory = fixture()
        inventory["fabric_topology"]["bus_nodes"][0]["bus_device"] = None
        with self.assertRaisesRegex(ValueError, "no resolved qcom,bus-dev parent"):
            SIMULATOR.simulate(trace, inventory)

    def test_flags_multiple_rscs_in_one_kernel_commit_transaction(self) -> None:
        trace, inventory = fixture()
        for node in inventory["fabric_topology"]["bus_nodes"]:
            for binding in node["bcm_bindings"]:
                if binding["node"] == "bcm-c":
                    binding["commit_rsc"] = {"node": "rsc-disp", "cell_id": 8001}
        result = SIMULATOR.simulate(trace, inventory)
        self.assertEqual(result["summary"]["simulated_updates"], 0)
        self.assertEqual(
            result["event_issues"][0]["issue"],
            "multiple_rscs_in_one_commit_transaction",
        )
        self.assertEqual(result["event_issues"][0]["rsc_nodes"], ["rsc-apps", "rsc-disp"])

    def test_observed_clock_domains_produce_candidate_vcd_grouping(self) -> None:
        trace, inventory = fixture()
        metadata = {
            "target": "sunfish",
            "metadata_source": {"result": "METADATA PASS"},
            "resources": {
                "BCM-A": {"address": "0x100", "clock_domain": 3},
                "BCM-B": {"address": "0x200", "clock_domain": 0},
                "BCM-C": {"address": "0x300", "clock_domain": 3},
            },
        }
        event = SIMULATOR.simulate(trace, inventory, metadata)["events"][0]
        self.assertEqual(
            event["vcd_grouped_bcm_touch_candidate"],
            [
                {"vcd_index": 0, "bcms": [{
                    "bcm_node": "bcm-b", "bcm_phandle": 11,
                    "cmd_db_name": "BCM-B",
                }]},
                {"vcd_index": 3, "bcms": [
                    {"bcm_node": "bcm-a", "bcm_phandle": 10,
                     "cmd_db_name": "BCM-A"},
                    {"bcm_node": "bcm-c", "bcm_phandle": 12,
                     "cmd_db_name": "BCM-C"},
                ]},
            ],
        )
        self.assertEqual(
            [row["observed_clock_domain"] for row in event["bcm_first_touch_order"]],
            [3, 0, 3],
        )
        self.assertFalse(SIMULATOR.simulate(trace, inventory, metadata)["summary"]
                         ["tcs_vcd_order_resolved"])

    def test_missing_observed_metadata_leaves_vcd_candidate_unresolved(self) -> None:
        trace, inventory = fixture()
        metadata = {
            "target": "sunfish",
            "resources": {"BCM-A": {"clock_domain": 3}},
        }
        event = SIMULATOR.simulate(trace, inventory, metadata)["events"][0]
        self.assertIsNone(event["vcd_grouped_bcm_touch_candidate"])
        self.assertEqual(
            [row["cmd_db_name"] for row in event["vcd_candidate_unresolved_bcms"]],
            ["BCM-B", "BCM-C"],
        )

    def test_rejects_invalid_observed_clock_domain(self) -> None:
        trace, inventory = fixture()
        metadata = {"target": "sunfish", "resources": {
            "BCM-A": {"clock_domain": 16},
        }}
        with self.assertRaisesRegex(ValueError, "invalid observed CMD_DB clock_domain"):
            SIMULATOR.simulate(trace, inventory, metadata)

    def test_empty_trace_produces_no_events_without_inventing_votes(self) -> None:
        trace, inventory = fixture()
        trace["request_updates"] = []
        result = SIMULATOR.simulate(trace, inventory)
        self.assertEqual(result["events"], [])
        self.assertEqual(result["summary"]["trace_request_updates"], 0)
        self.assertIn("complete live vote snapshot", result["scope"])


if __name__ == "__main__":
    unittest.main()
