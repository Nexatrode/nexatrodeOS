#!/usr/bin/env python3
"""Host-only checks for the read-only devbw sysfs snapshot decoder."""

from __future__ import annotations

import importlib.util
import unittest
from types import SimpleNamespace
from unittest.mock import call, patch
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "platforms" / "sunfish" / "capture-msm-bus-devfreq-state.py"
SPEC = importlib.util.spec_from_file_location("capture_msm_bus_devfreq_state", MODULE_PATH)
assert SPEC and SPEC.loader
CAPTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CAPTURE)


def profile() -> dict:
    return {
        "node": "/soc/qcom,cpu-llcc-ddr-bw",
        "runtime_name": "soc:qcom,cpu-llcc-ddr-bw",
        "governor": "performance",
        "active_only": True,
        "operating_points_v2": {
            "node": "/soc/ddr-bw-opp-table",
            "entries": [
                {"node": "opp-300", "request_ib_mibps": 1144, "status": "okay (default)"},
                {"node": "opp-451", "request_ib_mibps": 1720, "status": "okay (default)"},
                {"node": "opp-disabled", "request_ib_mibps": 2086, "status": "disabled"},
            ],
        },
        "paths": [
            {
                "path_index": 0,
                "source_id": 129,
                "destination_id": 512,
                "route": ["mas-llcc-mc", "slv-ebi"],
                "bcms": ["MC0", "ACV"],
            }
        ],
    }


VALUE_SEMANTICS = {
    "zero_average_governors": ["performance", "powersave"],
    "zero_average_source": "pinned source behavior",
    "dynamic_average_governors": ["bw_vbif"],
    "dynamic_average_source": "not available via sysfs",
    "dynamic_average_model": {
        "inputs": "VBIF ram_time and total_time counters",
        "formula": "roundup(..., 160) MiB/s",
    },
}


class DevfreqCaptureTests(unittest.TestCase):
    def test_remote_toybox_cat_error_is_unreadable_even_when_adb_exits_zero(self) -> None:
        with patch.object(
            CAPTURE.subprocess,
            "run",
            return_value=SimpleNamespace(
                returncode=0,
                stdout="cat: /sys/class/devfreq/example/cur_freq: Permission denied\n",
                stderr="",
            ),
        ):
            self.assertIsNone(
                CAPTURE.read_attribute("phone", "/sys/class/devfreq/example", "cur_freq")
            )

    def test_adb_stderr_is_not_treated_as_a_sysfs_value(self) -> None:
        with patch.object(
            CAPTURE.subprocess,
            "run",
            return_value=SimpleNamespace(returncode=0, stdout="1720\n", stderr="remote error"),
        ):
            self.assertIsNone(
                CAPTURE.read_attribute("phone", "/sys/class/devfreq/example", "cur_freq")
            )

    def test_readable_sysfs_scalar_is_trimmed(self) -> None:
        with patch.object(
            CAPTURE.subprocess,
            "run",
            return_value=SimpleNamespace(returncode=0, stdout="1720\n", stderr=""),
        ):
            self.assertEqual(
                CAPTURE.read_attribute("phone", "/sys/class/devfreq/example", "cur_freq"),
                "1720",
            )

    def test_maps_cur_freq_only_to_an_enabled_source_opp(self) -> None:
        row = CAPTURE.summarize_client(
            profile(),
            {
                "governor": "performance",
                "cur_freq": "1720\n",
                "available_frequencies": "1144 1720\n",
            },
            VALUE_SEMANTICS,
        )
        self.assertEqual(row["status"], "complete")
        self.assertEqual(row["current_ib_mibps"], 1720)
        self.assertTrue(row["available_frequencies_match_opp_table"])
        self.assertTrue(row["cur_freq_matches_opp_table"])
        self.assertEqual(row["opp_ib_mibps"], [1144, 1720])
        self.assertEqual(row["average_request_mibps"], 0)
        self.assertIsNone(row["average_request_model"])
        self.assertEqual(row["request_profile_status"], "complete")
        self.assertEqual(
            row["route_contributions"][0]["contexts"],
            {
                "active_only": {"average": 0, "peak": 1720 * (1 << 20)},
                "dual": {"average": 0, "peak": 0},
            },
        )

    def test_does_not_infer_an_opp_for_an_unmatched_current_value(self) -> None:
        row = CAPTURE.summarize_client(
            profile(),
            {
                "governor": "performance",
                "cur_freq": "9000",
                "available_frequencies": "1144 1720",
            },
            VALUE_SEMANTICS,
        )
        self.assertIsNone(row["current_ib_mibps"])
        self.assertTrue(row["available_frequencies_match_opp_table"])
        self.assertFalse(row["cur_freq_is_available"])
        self.assertFalse(row["cur_freq_matches_opp_table"])

    def test_empty_available_frequency_list_is_not_a_match(self) -> None:
        row = CAPTURE.summarize_client(
            profile(),
            {
                "governor": "performance",
                "cur_freq": "0",
                "available_frequencies": "",
            },
            VALUE_SEMANTICS,
        )
        self.assertFalse(row["available_frequencies_match_opp_table"])
        self.assertFalse(row["cur_freq_is_available"])

    def test_marks_unreadable_attributes_as_partial_without_failing_other_rows(self) -> None:
        row = CAPTURE.summarize_client(
            profile(),
            {
                "governor": "performance",
                "cur_freq": None,
                "available_frequencies": "1144 1720",
            },
            VALUE_SEMANTICS,
        )
        self.assertEqual(row["status"], "partial")
        self.assertIn("cur_freq", row["unreadable_attributes"])
        self.assertIsNone(row["current_ib_mibps"])

    def test_bw_vbif_keeps_dynamic_average_unknown(self) -> None:
        gpu = profile()
        gpu["governor"] = "bw_vbif"
        row = CAPTURE.summarize_client(
            gpu,
            {
                "governor": "bw_vbif",
                "cur_freq": "1720",
                "available_frequencies": "1144 1720",
            },
            VALUE_SEMANTICS,
        )
        self.assertIsNone(row["average_request_mibps"])
        self.assertEqual(row["average_request_source"], "not available via sysfs")
        self.assertEqual(
            row["average_request_model"], VALUE_SEMANTICS["dynamic_average_model"]
        )
        self.assertEqual(row["request_profile_status"], "partial")
        self.assertEqual(
            row["route_contributions"][0]["contexts"]["active_only"]["peak"],
            1720 * (1 << 20),
        )
        self.assertIsNone(
            row["route_contributions"][0]["contexts"]["active_only"]["average"]
        )

    def test_changed_governor_disables_source_inference_for_average_vote(self) -> None:
        row = CAPTURE.summarize_client(
            profile(),
            {
                "governor": "bw_vbif",
                "cur_freq": "1720",
                "available_frequencies": "1144 1720",
            },
            VALUE_SEMANTICS,
        )
        self.assertFalse(row["governor_matches_profile"])
        self.assertIsNone(row["average_request_mibps"])
        self.assertIsNone(row["average_request_model"])
        self.assertEqual(
            row["average_request_source"],
            "observed governor is not source-matched to the inventory",
        )
        self.assertEqual(row["request_profile_status"], "partial")

    def test_device_parser_keeps_all_rows_for_exact_target_selection(self) -> None:
        self.assertEqual(
            CAPTURE.parse_devices(
                "List of devices attached\nunauthorized\tunauthorized\n"
                "phone-serial\tdevice product:sunfish model:Pixel_4a\n"
            ),
            [("unauthorized", "unauthorized"), ("phone-serial", "device")],
        )

    def test_capture_uses_exact_sunfish_serial_and_only_read_commands(self) -> None:
        inventory = {
            "dtb_sha256": "dtb-hash",
            "devbw_value_semantics": VALUE_SEMANTICS,
            "devbw_clients": [profile()],
        }
        values = {
            "governor": "performance",
            "cur_freq": "1720",
            "available_frequencies": "1144 1720",
        }
        with (
            patch.object(
                CAPTURE,
                "adb_text",
                side_effect=[
                    "List of devices attached\nphone-serial\tdevice product:sunfish\n",
                    "sunfish",
                ],
            ) as adb,
            patch.object(
                CAPTURE,
                "read_attribute",
                side_effect=lambda _serial, _path, attribute: values[attribute],
            ) as read,
        ):
            result = CAPTURE.capture(inventory)

        self.assertEqual(result["summary"]["complete"], 1)
        self.assertEqual(result["clients"][0]["current_ib_mibps"], 1720)
        self.assertEqual(
            adb.call_args_list,
            [
                call("devices", "-l"),
                call("-s", "phone-serial", "shell", "getprop", "ro.product.device"),
            ],
        )
        self.assertEqual(
            [item.args[2] for item in read.call_args_list],
            ["governor", "cur_freq", "available_frequencies"],
        )

    def test_capture_refuses_multiple_devices_and_wrong_product_before_reads(self) -> None:
        inventory = {
            "devbw_value_semantics": VALUE_SEMANTICS,
            "devbw_clients": [profile()],
        }
        with patch.object(
            CAPTURE,
            "adb_text",
            return_value=(
                "List of devices attached\nfirst\tdevice\nsecond\tdevice\n"
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, "exactly one authorized"):
                CAPTURE.capture(inventory)

        with (
            patch.object(
                CAPTURE,
                "adb_text",
                side_effect=["List of devices attached\nphone\tdevice\n", "other-device"],
            ),
            patch.object(CAPTURE, "read_attribute") as read,
        ):
            with self.assertRaisesRegex(RuntimeError, "expected Sunfish"):
                CAPTURE.capture(inventory)
            read.assert_not_called()


if __name__ == "__main__":
    unittest.main()
