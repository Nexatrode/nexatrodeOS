import importlib.util
from pathlib import Path
import unittest
from unittest.mock import call, patch


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "wifi_trace_runner", ROOT / "platforms/sunfish/run-msm-bus-wifi-comparison.py"
)
assert SPEC and SPEC.loader
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


DISABLED_RECORDS = [
    'func update_request_adhoc =_ "..."',
    'func set_bw =_ "..."',
]


class DynamicDebugCleanupTests(unittest.TestCase):
    def test_attempts_both_disables_and_verifies_readback_after_first_command_error(self):
        with (
            patch.object(
                RUNNER,
                "adb",
                side_effect=[RuntimeError("transient adb error"), ""],
            ) as adb,
            patch.object(RUNNER, "control_records", return_value=DISABLED_RECORDS),
        ):
            self.assertEqual(RUNNER.restore_dynamic_debug(), DISABLED_RECORDS)

        self.assertEqual(
            adb.call_args_list,
            [
                call("shell", f"echo 'func update_request_adhoc -p' > {RUNNER.CONTROL}"),
                call("shell", f"echo 'func set_bw -p' > {RUNNER.CONTROL}"),
            ],
        )

    def test_reports_callsite_still_enabled_after_both_disable_attempts(self):
        active_records = [
            'func update_request_adhoc =p "..."',
            'func set_bw =_ "..."',
        ]
        with (
            patch.object(RUNNER, "adb", return_value="") as adb,
            patch.object(RUNNER, "control_records", return_value=active_records),
        ):
            with self.assertRaisesRegex(RuntimeError, "print flags remain enabled"):
                RUNNER.restore_dynamic_debug()

        self.assertEqual(adb.call_count, 2)

    def test_reports_when_cleanup_state_cannot_be_read(self):
        with (
            patch.object(RUNNER, "adb", return_value="") as adb,
            patch.object(
                RUNNER,
                "control_records",
                side_effect=RuntimeError("adb disconnected"),
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, "could not verify dynamic-debug cleanup"):
                RUNNER.restore_dynamic_debug()

        self.assertEqual(adb.call_count, 2)


if __name__ == "__main__":
    unittest.main()
