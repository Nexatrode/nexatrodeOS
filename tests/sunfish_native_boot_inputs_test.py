"""Host-only input preparation checks; synthetic data, no private fixtures."""
import hashlib
import importlib.util
import json
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "native_inputs", ROOT / "platforms/sunfish/prepare-native-boot-inputs.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NativeInputsTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.output = self.root / "private"
        self.manifest_path = self.root / "source.json"
        self.wlan = self.root / "wlan.mbn"
        self.wlan.write_bytes(b"synthetic WLAN test input")
        self.manifest = {
            "phone_serial": "test-phone", "device_binding_sha256": "51" * 32,
            "linux_boot_id": "bf46e35a-0634-4ae7-86d1-86c93d2d1ed3",
            "storage_mode": "RAM shadow, no persistent writes", "sources": [],
        }
        for i, (name, size) in enumerate(MODULE.PARTITIONS):
            data = bytes([i + 1]) * size
            path = self.root / (name + ".bin")
            path.write_bytes(data)
            self.manifest["sources"].append({
                "partition": name, "path": str(path), "bytes": size,
                "sha256": hashlib.sha256(data).hexdigest(),
            })

    def prepare(self, serial="test-phone", binding="51" * 32):
        self.manifest_path.write_text(json.dumps(self.manifest))
        # Exercise preparation with redistributable synthetic bytes. The
        # separate private integration check uses the unmodified real profile.
        with patch.object(MODULE, "WLAN_BYTES", self.wlan.stat().st_size), \
             patch.object(MODULE, "WLAN_SHA256", hashlib.sha256(self.wlan.read_bytes()).hexdigest()):
            return MODULE.prepare(self.manifest_path, self.wlan, serial, binding, self.output)

    def test_private_output_has_exact_copies_and_explicit_volatile_configuration(self):
        report = self.prepare()
        self.assertEqual(stat.S_IMODE(self.output.stat().st_mode), 0o700)
        for name, record in report["files"].items():
            path = self.output / name
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), record["sha256"])
        module = (self.output / "private_boot_inputs.rs").read_text()
        self.assertEqual(module.count("writable: true, durable: false"), 4)
        self.assertEqual(module.count("include_bytes!"), 5)
        self.assertNotIn(str(self.root / "modemst1.bin"), module)

    def test_wrong_phone_and_binding_create_no_output(self):
        for serial, binding in [("other-phone", "51" * 32), ("test-phone", "52" * 32),
                                ("test-phone", "00" * 32)]:
            with self.subTest(serial=serial, binding=binding), self.assertRaises(ValueError):
                self.prepare(serial, binding)
            self.assertFalse(self.output.exists())

    def test_corrupt_last_efs_file_creates_no_partial_output(self):
        last = Path(self.manifest["sources"][-1]["path"])
        data = bytearray(last.read_bytes())
        data[-1] ^= 1
        last.write_bytes(data)
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            self.prepare()
        self.assertFalse(self.output.exists())

    def test_wrong_partition_order_geometry_or_persistence_is_rejected(self):
        original = json.dumps(self.manifest)
        for mutation in ("order", "size", "persistence"):
            self.manifest = json.loads(original)
            if mutation == "order":
                self.manifest["sources"].reverse()
            elif mutation == "size":
                self.manifest["sources"][0]["bytes"] -= 512
            else:
                self.manifest["storage_mode"] = "persistent"
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.prepare()
            self.assertFalse(self.output.exists())

    def test_wrong_wlan_digest_creates_no_partial_output(self):
        self.manifest_path.write_text(json.dumps(self.manifest))
        with patch.object(MODULE, "WLAN_BYTES", self.wlan.stat().st_size), \
             patch.object(MODULE, "WLAN_SHA256", "12" * 32), \
             self.assertRaisesRegex(ValueError, "digest mismatch"):
            MODULE.prepare(self.manifest_path, self.wlan, "test-phone", "51" * 32, self.output)
        self.assertFalse(self.output.exists())

    def test_existing_output_is_retained(self):
        self.output.mkdir()
        marker = self.output / "retained"
        marker.write_text("do not replace")
        with self.assertRaisesRegex(ValueError, "new directory"):
            self.prepare()
        self.assertEqual(marker.read_text(), "do not replace")
        self.assertEqual(list(self.output.iterdir()), [marker])


if __name__ == "__main__":
    unittest.main()
