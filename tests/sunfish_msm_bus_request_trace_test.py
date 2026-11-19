#!/usr/bin/env python3
"""Host-only checks for Sunfish MSM-bus request-trace auditing."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "platforms" / "sunfish" / "audit-msm-bus-request-trace.py"
SPEC = importlib.util.spec_from_file_location("audit_msm_bus_request_trace", MODULE_PATH)
assert SPEC and SPEC.loader
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class MsmBusRequestTraceTests(unittest.TestCase):
    def test_keeps_kernel_uptime_and_summarizes_half_open_windows(self) -> None:
        trace = AUDIT.parse_trace("""\
[  1.000000] register_client_adhoc:find path.src 90 dest 512
[  1.100000] register_client_adhoc:Client handle 10 ipa
[  1.500000] update_request_adhoc: cl: 10 index: 1 curr: -1 num_paths: 1
[  2.000000] devbw soc:qcom,ipa: BW MBps: AB: 4 IB: 8
[  2.500000] update_request_adhoc: cl: 10 index: 0 curr: 1 num_paths: 1
""")
        self.assertEqual(trace["registrations"][0]["registered_at_seconds"], 1.1)
        self.assertEqual(trace["request_updates"][0]["kernel_time_seconds"], 1.5)
        self.assertEqual(trace["devbw_samples"][0]["kernel_time_seconds"], 2.0)

        windows = AUDIT.parse_time_windows(["wifi_off:1.0:2.5", "wifi_on:2.5:3.0"])
        summary = AUDIT.summarize_time_windows(trace, windows)
        self.assertEqual(summary[0]["request_update_count"], 1)
        self.assertEqual(summary[0]["updates_by_client"]["ipa#10"]["transitions"], {"-1->1": 1})
        self.assertEqual(summary[0]["devbw_sample_count"], 1)
        self.assertEqual(summary[0]["devbw_by_device"]["soc:qcom,ipa"]["instantaneous_mb_s_max"], 8)
        self.assertEqual(summary[1]["request_update_count"], 1)
        self.assertEqual(summary[1]["devbw_sample_count"], 0)

    def test_rejects_invalid_or_ambiguous_time_windows(self) -> None:
        for windows in (
            ["bad"],
            ["zero:1:1"],
            ["negative:-1:2"],
            ["nan:NaN:3"],
            ["same:1:2", "same:2:3"],
        ):
            with self.subTest(windows=windows), self.assertRaises(ValueError):
                AUDIT.parse_time_windows(windows)

    def test_reconstructs_transitions_and_keeps_devbw_values(self) -> None:
        trace = """\
03-30 12:00:00.001 0 0 E : c6 1 register_client_adhoc:find path.src 1 dest 2
03-30 12:00:00.002 0 0 E : c6 1 register_client_adhoc:find path.src 2 dest 3
03-30 12:00:00.003 0 0 E : c6 1 register_client_adhoc:Client handle 4 ipa
03-30 12:00:00.004 0 0 D : devbw soc:qcom,cpu-llcc-ddr-bw: BW MBps: AB: 90 IB: 180
03-30 12:00:00.005 0 0 D : msm_bus_arb_rpmh: update_request_adhoc: cl: 4 index: 1 curr: -1 num_paths: 2
03-30 12:00:00.006 0 0 D : msm_bus_arb_rpmh: update_request_adhoc: cl: 4 index: 0 curr: 1 num_paths: 2
03-30 12:00:00.007 0 0 D : update_request_adhoc: Not updating client request idx 0 unchanged
"""
        result = AUDIT.parse_trace(trace)
        self.assertEqual(result["summary"]["registered_handles"], 1)
        self.assertEqual(result["summary"]["request_updates"], 2)
        self.assertEqual(result["summary"]["anomalies"], 0)
        registration = result["registrations"][0]
        self.assertEqual(registration["name"], "ipa")
        self.assertEqual(registration["final_index"], 0)
        self.assertEqual(len(registration["paths"]), 2)
        self.assertEqual(result["devbw_samples"][0]["device_name"], "soc:qcom,cpu-llcc-ddr-bw")
        self.assertEqual(result["devbw_samples"][0]["average_mb_s"], 90)
        self.assertEqual(result["devbw_samples"][0]["instantaneous_mb_s"], 180)
        self.assertEqual(result["summary"]["unchanged_index_notes_without_handle"], 1)

    def test_surfaces_bad_priors_path_counts_and_unmatched_handles(self) -> None:
        trace = """\
register_client_adhoc:find path.src 1 dest 2
register_client_adhoc:Client handle 7 usb0
update_request_adhoc: cl: 7 index: 1 curr: 0 num_paths: 2
update_request_adhoc: cl: 99 index: 1 curr: -1 num_paths: 1
"""
        result = AUDIT.parse_trace(trace)
        kinds = {item["kind"] for item in result["anomalies"]}
        self.assertEqual(kinds, {
            "prior_index_mismatch",
            "path_count_mismatch",
            "update_before_or_without_registration",
        })
        self.assertEqual(result["summary"]["matched_updates"], 1)
        self.assertEqual(result["summary"]["anomalies"], 3)

    def test_keeps_failed_and_orphan_registration_paths(self) -> None:
        trace = """\
register_client_adhoc:find path.src 3 dest 4
register_client_adhoc:Failed to find path.src 3 dest 4
register_client_adhoc:find path.src 5 dest 6
"""
        result = AUDIT.parse_trace(trace)
        self.assertEqual(result["summary"]["failed_path_logs"], 1)
        self.assertEqual(result["orphan_paths"], [{"source_id": 5, "destination_id": 6}])
        self.assertEqual(result["summary"]["anomalies"], 1)

    def test_joins_static_kbps_case_and_maps_dual_context(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 2
register_client_adhoc:Client handle 8 pil-modem
update_request_adhoc: cl: 8 index: 1 curr: -1 num_paths: 1
""")
        inventory = {"clients": [{
            "name": "pil-modem",
            "node": "/soc/qcom,mss",
            "vector_property": "qcom,msm-bus,vectors-KBps",
            "active_only": False,
            "cases": [
                {"name": "OFF", "paths": [{
                    "path_index": 0, "source_id": 1, "destination_id": 2,
                    "average": 0, "instantaneous": 0, "route": ["n1"], "bcms": ["MC0"],
                }]},
                {"name": "ON", "paths": [{
                    "path_index": 0, "source_id": 1, "destination_id": 2,
                    "average": 7, "instantaneous": 19, "route": ["n1"], "bcms": ["MC0"],
                }]},
            ],
        }]}
        joined = AUDIT.join_static_inventory(trace, inventory)
        client = joined["clients"][0]
        self.assertEqual(client["status"], "matched_static")
        self.assertEqual(client["case_name"], "ON")
        self.assertEqual(client["paths"][0]["active"]["average_bytes_per_second"], 0)
        self.assertEqual(client["paths"][0]["dual"]["average_bytes_per_second"], 7000)
        self.assertEqual(client["paths"][0]["dual"]["instantaneous_bytes_per_second"], 19000)

    def test_joins_each_static_request_transition_to_old_and_new_vectors(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 2
register_client_adhoc:Client handle 8 pil-modem
update_request_adhoc: cl: 8 index: 1 curr: -1 num_paths: 1
update_request_adhoc: cl: 8 index: 0 curr: 1 num_paths: 1
""")
        inventory = {"clients": [{
            "name": "pil-modem",
            "node": "/soc/qcom,mss",
            "vector_property": "qcom,msm-bus,vectors-KBps",
            "active_only": False,
            "cases": [
                {"name": "OFF", "paths": [{
                    "path_index": 0, "source_id": 1, "destination_id": 2,
                    "average": 0, "instantaneous": 0,
                }]},
                {"name": "ON", "paths": [{
                    "path_index": 0, "source_id": 1, "destination_id": 2,
                    "average": 7, "instantaneous": 19,
                }]},
            ],
        }]}
        joined = AUDIT.join_static_request_transitions(trace, inventory)
        first, second = joined["updates"]
        self.assertEqual(joined["summary"], {"known_transition": 2})
        self.assertEqual(first["old_state"]["status"], "inactive")
        self.assertEqual(first["new_state"]["case_name"], "ON")
        self.assertEqual(
            first["new_state"]["paths"][0]["dual"]["average_bytes_per_second"],
            7000,
        )
        self.assertEqual(second["old_state"]["case_name"], "ON")
        self.assertEqual(second["new_state"]["case_name"], "OFF")

    def test_transition_join_keeps_new_vector_but_flags_bad_prior(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 2
register_client_adhoc:Client handle 7 usb0
update_request_adhoc: cl: 7 index: 1 curr: 0 num_paths: 1
""")
        inventory = {"clients": [{
            "name": "usb0",
            "node": "/usb0",
            "vector_property": "qcom,msm-bus,vectors-KBps",
            "active_only": True,
            "cases": [
                {"name": "OFF", "paths": [{
                    "source_id": 1, "destination_id": 2,
                    "average": 0, "instantaneous": 0,
                }]},
                {"name": "ON", "paths": [{
                    "source_id": 1, "destination_id": 2,
                    "average": 10, "instantaneous": 20,
                }]},
            ],
        }]}
        joined = AUDIT.join_static_request_transitions(trace, inventory)
        update = joined["updates"][0]
        self.assertEqual(update["status"], "reported_prior_index_mismatch")
        self.assertEqual(update["old_state"]["case_name"], "OFF")
        self.assertEqual(update["new_state"]["case_name"], "ON")

    def test_transition_join_does_not_call_old_out_of_range_state_known(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 2
register_client_adhoc:Client handle 7 usb0
update_request_adhoc: cl: 7 index: 8 curr: -1 num_paths: 1
update_request_adhoc: cl: 7 index: 1 curr: 8 num_paths: 1
""")
        inventory = {"clients": [{
            "name": "usb0",
            "node": "/usb0",
            "vector_property": "qcom,msm-bus,vectors-KBps",
            "active_only": True,
            "cases": [
                {"name": "OFF", "paths": [{
                    "source_id": 1, "destination_id": 2,
                    "average": 0, "instantaneous": 0,
                }]},
                {"name": "ON", "paths": [{
                    "source_id": 1, "destination_id": 2,
                    "average": 10, "instantaneous": 20,
                }]},
            ],
        }]}
        joined = AUDIT.join_static_request_transitions(trace, inventory)
        first, second = joined["updates"]
        self.assertEqual(first["status"], "usecase_index_out_of_range")
        self.assertEqual(second["status"], "old_state_unresolved")
        self.assertEqual(second["new_state"]["case_name"], "ON")

    def test_attaches_ordered_physical_bcm_and_rsc_identities(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 2
register_client_adhoc:Client handle 8 pil-modem
update_request_adhoc: cl: 8 index: 1 curr: -1 num_paths: 1
""")
        inventory = {
            "clients": [{
                "name": "pil-modem",
                "node": "/soc/qcom,mss",
                "vector_property": "qcom,msm-bus,vectors-KBps",
                "active_only": False,
                "cases": [
                    {"name": "OFF", "paths": [{
                        "path_index": 0, "source_id": 1, "destination_id": 2,
                        "average": 0, "instantaneous": 0,
                        "route": ["n1", "n2"], "bcms": ["MC0", "MC0"],
                    }]},
                    {"name": "ON", "paths": [{
                        "path_index": 0, "source_id": 1, "destination_id": 2,
                        "average": 7, "instantaneous": 19,
                        "route": ["n1", "n2"], "bcms": ["MC0", "MC0"],
                    }]},
                ],
            }],
            "fabric_topology": {"bus_nodes": [
                {"node": "n1", "cell_id": 10, "bcm_bindings": [{
                    "binding_index": 0, "node": "bcm-mc0", "cell_id": 7000,
                    "cmd_db_name": "MC0",
                    "commit_rsc": {"node": "rsc-apps", "cell_id": 8000},
                }]},
                {"node": "n2", "cell_id": 20, "bcm_bindings": [{
                    "binding_index": 0, "node": "bcm-mc0-display", "cell_id": 27000,
                    "cmd_db_name": "MC0",
                    "commit_rsc": {"node": "rsc-disp", "cell_id": 8001},
                }]},
            ]},
        }

        client = AUDIT.join_static_inventory(trace, inventory)["clients"][0]
        bindings = client["paths"][0]["bcm_bindings"]
        self.assertEqual([(row["route_index"], row["bcm_node"], row["rsc_node"]) for row in bindings], [
            (0, "bcm-mc0", "rsc-apps"),
            (1, "bcm-mc0-display", "rsc-disp"),
        ])

    def test_ambiguous_static_duplicates_are_not_guessed(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 2
register_client_adhoc:Client handle 8 apps_smmu
update_request_adhoc: cl: 8 index: 1 curr: -1 num_paths: 1
""")
        def client(node: str, instant: int) -> dict:
            return {
                "name": "apps_smmu",
                "node": node,
                "vector_property": "qcom,msm-bus,vectors-KBps",
                "active_only": True,
                "cases": [
                    {"name": "OFF", "paths": [{"source_id": 1, "destination_id": 2, "average": 0, "instantaneous": 0}]},
                    {"name": "ON", "paths": [{"source_id": 1, "destination_id": 2, "average": 0, "instantaneous": instant}]},
                ],
            }
        joined = AUDIT.join_static_inventory(trace, {"clients": [client("/a", 10), client("/b", 20)]})
        result = joined["clients"][0]
        self.assertEqual(result["status"], "ambiguous_static_match")
        self.assertNotIn("paths", result)
        self.assertEqual(result["candidate_nodes"], ["/a", "/b"])

    def test_joins_exact_source_backed_scm_pas_profile(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 125 dest 512
register_client_adhoc:Client handle 12 scm_pas
update_request_adhoc: cl: 12 index: 1 curr: -1 num_paths: 1
""")
        joined = AUDIT.join_static_inventory(trace, {"clients": []})
        client = joined["clients"][0]
        self.assertEqual(client["status"], "matched_source_profile")
        self.assertEqual(client["case_name"], "PAS_LOAD")
        self.assertEqual(client["active_only"], False)
        self.assertEqual(client["paths"][0]["dual"]["average_bytes_per_second"], 393_600_000)
        self.assertEqual(client["paths"][0]["dual"]["instantaneous_bytes_per_second"], 3_936_000_000)
        self.assertEqual(client["paths"][0]["active"]["average_bytes_per_second"], 0)

    def test_joins_exact_source_backed_sde_reg_profile_and_refuses_other_route(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 590
register_client_adhoc:Client handle 26 sde_reg
update_request_adhoc: cl: 26 index: 3 curr: -1 num_paths: 1
""")
        joined = AUDIT.join_static_inventory(trace, {"clients": []})
        client = joined["clients"][0]
        self.assertEqual(client["status"], "matched_source_profile")
        self.assertEqual(client["case_name"], "80_MHZ")
        self.assertEqual(client["paths"][0]["active"]["instantaneous_bytes_per_second"], 640_000_000)
        self.assertEqual(client["paths"][0]["dual"]["instantaneous_bytes_per_second"], 0)

        wrong_route = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 512
register_client_adhoc:Client handle 26 sde_reg
update_request_adhoc: cl: 26 index: 3 curr: -1 num_paths: 1
""")
        unmatched = AUDIT.join_static_inventory(wrong_route, {"clients": []})
        self.assertEqual(unmatched["clients"][0]["status"], "no_static_match")

    def test_joins_devbw_sample_to_route_and_reconstructs_per_path_vote(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 2
register_client_adhoc:find path.src 3 dest 4
register_client_adhoc:Client handle 9 soc:qcom,cpu-llcc-ddr-bw
devbw soc:qcom,cpu-llcc-ddr-bw: BW MBps: AB: 101 IB: 200
update_request_adhoc: cl: 9 index: 1 curr: -1 num_paths: 2
""")
        inventory = {"devbw_clients": [{
            "node": "/soc/qcom,cpu-llcc-ddr-bw",
            "runtime_name": "soc:qcom,cpu-llcc-ddr-bw",
            "active_only": True,
            "paths": [
                {"path_index": 0, "source_id": 1, "destination_id": 2, "route": ["r1"], "bcms": ["BCM_A"]},
                {"path_index": 1, "source_id": 3, "destination_id": 4, "route": ["r2"], "bcms": ["BCM_B"]},
            ],
        }]}
        joined = AUDIT.join_devbw_samples(trace, inventory)
        request = joined["samples"][0]
        self.assertEqual(request["status"], "matched_devbw_request_update")
        self.assertEqual(request["buffer_index"], 1)
        self.assertEqual(request["paths"][0]["active"]["average_bytes_per_second_per_path"], 50 * (1 << 20))
        self.assertEqual(request["paths"][1]["active"]["instantaneous_bytes_per_second_per_path"], 200 * (1 << 20))
        self.assertEqual(request["paths"][0]["dual"]["average_bytes_per_second"], 0)

    def test_devbw_join_refuses_route_mismatch_and_missing_update(self) -> None:
        trace = AUDIT.parse_trace("""\
register_client_adhoc:find path.src 1 dest 2
register_client_adhoc:Client handle 9 soc:qcom,cpu-llcc-ddr-bw
devbw soc:qcom,cpu-llcc-ddr-bw: BW MBps: AB: 1 IB: 2
""")
        inventory = {"devbw_clients": [{
            "node": "/soc/qcom,cpu-llcc-ddr-bw",
            "runtime_name": "soc:qcom,cpu-llcc-ddr-bw",
            "active_only": False,
            "paths": [{"path_index": 0, "source_id": 1, "destination_id": 3, "route": ["r"], "bcms": ["B"]}],
        }]}
        joined = AUDIT.join_devbw_samples(trace, inventory)
        self.assertEqual(joined["samples"][0]["status"], "runtime_route_mismatch")

        inventory["devbw_clients"][0]["paths"][0]["destination_id"] = 2
        joined = AUDIT.join_devbw_samples(trace, inventory)
        self.assertEqual(joined["samples"][0]["status"], "no_following_request_update")


if __name__ == "__main__":
    unittest.main()
