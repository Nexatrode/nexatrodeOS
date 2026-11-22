"""Check release selection at the public wallet builder without a cross compiler."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
BOARD = ROOT / "experiments/hvf-smp"


class AcceleratedProfileTest(unittest.TestCase):
    def build_options(self, *flags):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            cargo = tmp / "cargo"
            cargo.write_text("""#!/bin/sh
set -eu
dir="$CARGO_TARGET_DIR/aarch64-sel4-microkit/release"
mkdir -p "$dir"
touch "$dir/rust_wallet_app.elf" "$dir/rust_wallet_range_worker.elf"
printf '%s\\n' "$NEXAWAL_BENCH_HEALTHY_MS" "$@"
""")
            cargo.chmod(0o700)
            env = dict(os.environ, PATH=str(tmp) + os.pathsep + os.environ["PATH"],
                       CARGO_TARGET_DIR=str(tmp / "cargo-target"))
            output = subprocess.check_output([ROOT / "experiments/ghc-microkit/build_wallet_scan_workers.sh",
                                              tmp, tmp / "wallet", *flags], env=env, text=True)
            return output.splitlines()

    def test_ordinary_rebuild_selects_measured_flags(self):
        args = self.build_options()
        self.assertEqual(args, self.build_options("--handoff-prime-m1-100"))
        self.assertEqual(args[0], "100")
        features = args[args.index("--features") + 1].split(",")
        self.assertIn("wallet-handoff-prime", features)
        self.assertIn("walletcore-core/utm-range-74", features)
        self.assertIn("wallet-wide-rx", features)
        self.assertIn('profile.release.package.curve25519-dalek.rustflags=["-Ztune-cpu=apple-m1"]', args)

    def test_control_keeps_its_own_settings(self):
        args = self.build_options("--utm-74", "--benchmark", "--healthy-ms=50")
        self.assertEqual(args[0], "50")
        self.assertNotIn("wallet-handoff-prime", args[args.index("--features") + 1].split(","))
        self.assertNotIn("profile-rustflags", args)

    def test_host_and_guest_use_all_four_lanes(self):
        lock = json.loads((BOARD / "accelerated.lock.json").read_text())
        root = ET.parse(BOARD / "accelerated.system")
        self.assertEqual(lock["cpus"], 4)
        self.assertEqual(root.find("protection_domain[@name='wallet_app']").get("cpu", "0"), "0")
        workers = [int(pd.get("cpu")) for pd in root.iter("protection_domain")
                   if pd.find("program_image[@path='rust_wallet_range_worker.elf']") is not None]
        self.assertEqual(sorted(workers), list(range(1, lock["cpus"])))


if __name__ == "__main__":
    unittest.main()
