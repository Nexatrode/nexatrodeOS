#!/usr/bin/env python3
"""Check runtime stack declaration against the actual mapped stack frames."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "runtime_stack_audit", ROOT / "platforms/sunfish/validate-native-mpss-runtime.py"
)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def fixture():
    objects = [
        {"name": f"frame_native_startup_preflight_stack_{i:09d}",
         "object": {"Frame": {"size_bits": 12}}}
        for i in range(512)
    ]
    rights = {"read": True, "write": True, "grant": False, "grant_reply": False}
    objects.append({"name": "pt_native_startup_preflight_vaddr_0x7ffffffff000", "object": {"PageTable": {"slots": [
        {"slot": i, "cap": {"Frame": {"object": i, "rights": rights.copy(),
                                       "cached": True, "executable": False}}}
        for i in range(512)
    ]}}})
    objects.append({"name": "pd_native_startup_preflight", "object": {"PageTable": {"slots": [
        {"slot": 0, "cap": {"PageTable": {"object": 512}}}
    ]}}})
    objects.append({"name": "pud_native_startup_preflight", "object": {"PageTable": {"slots": [
        {"slot": 0, "cap": {"PageTable": {"object": 513}}}
    ]}}})
    objects.append({"name": "pgd_native_startup_preflight", "object": {"PageTable": {"slots": [
        {"slot": 0, "cap": {"PageTable": {"object": 514}}}
    ]}}})
    objects.append({"name": "tcb_native_startup_preflight", "object": {"Tcb": {"slots": [
        {"slot": 1, "cap": {"PageTable": {"object": 515}}}
    ], "extra": {"sp": 0x200000}}}})
    return {"objects": objects}


class RuntimeStackTests(unittest.TestCase):
    def check(self, size, model):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".system") as f:
            f.write(f'<system><protection_domain name="native_startup_preflight" stack_size="{size}"/></system>')
            f.flush()
            return audit.audit_runtime_stack(f.name, model)

    def test_full_mapped_stack_passes(self):
        self.check("0x200000", fixture())

    def test_one_megabyte_stack_is_rejected(self):
        with self.assertRaises(ValueError):
            self.check("0x100000", fixture())

    def test_missing_stack_frame_mapping_is_rejected(self):
        model = fixture()
        model["objects"][512]["object"]["PageTable"]["slots"].pop()
        with self.assertRaises(ValueError):
            self.check("0x200000", model)

    def test_executable_stack_frame_is_rejected(self):
        model = fixture()
        model["objects"][512]["object"]["PageTable"]["slots"][0]["cap"]["Frame"]["executable"] = True
        with self.assertRaises(ValueError):
            self.check("0x200000", model)

    def test_stack_mapped_only_in_another_domain_is_rejected(self):
        model = fixture()
        model["objects"][512]["name"] = "pt_other_domain_vaddr_0x7ffffffff000"
        with self.assertRaises(ValueError):
            self.check("0x200000", model)

    def test_named_stack_table_not_reachable_from_tcb_is_rejected(self):
        model = fixture()
        model["objects"][515]["object"]["PageTable"]["slots"].clear()
        with self.assertRaises(ValueError):
            self.check("0x200000", model)

    def test_tcb_with_another_root_is_rejected(self):
        model = fixture()
        model["objects"][516]["object"]["Tcb"]["slots"][0]["cap"]["PageTable"]["object"] = 512
        with self.assertRaises(ValueError):
            self.check("0x200000", model)

    def test_missing_virtual_page_in_stack_range_is_rejected(self):
        model = fixture()
        model["objects"][512]["object"]["PageTable"]["slots"][0]["slot"] = 511
        with self.assertRaises(ValueError):
            self.check("0x200000", model)


if __name__ == "__main__":
    unittest.main()
