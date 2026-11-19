#!/usr/bin/env python3
"""Build and run the host-only Sunfish QCACLD-to-sDDF packet-copy tests."""

from pathlib import Path
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
SDDF = ROOT / ".cache/sddf-personal-os"
SDDF_REVISION = "df783e83cd29b23303bd6c172a6736f34a1301a8"
SDDF_HEADERS = (
    "include/sddf/network/config.h",
    "include/sddf/network/queue.h",
    "include/sddf/network/constants.h",
)


def main():
    compiler = shutil.which("cc")
    if compiler is None:
        raise SystemExit("a C compiler (`cc`) is required")
    if not SDDF.is_dir():
        raise SystemExit(f"pinned sDDF checkout is missing: {SDDF}")

    revision = subprocess.run(
        ["git", "-C", str(SDDF), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if revision != SDDF_REVISION:
        raise SystemExit(f"sDDF revision mismatch: expected {SDDF_REVISION}, got {revision}")
    dirty = subprocess.run(
        ["git", "-C", str(SDDF), "status", "--porcelain", "--", *SDDF_HEADERS],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if dirty:
        raise SystemExit("audited sDDF network headers have local changes")

    with tempfile.TemporaryDirectory(prefix="sunfish-wlan-net-") as temporary:
        binary = Path(temporary) / "sunfish_wlan_net_bridge_test"
        subprocess.run(
            [
                compiler,
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Wno-sign-compare",
                "-Wno-missing-field-initializers",
                "-DCONFIG_ARCH_AARCH64",
                "-I",
                str(ROOT / "tests/sddf_stubs"),
                "-I",
                str(SDDF / "include"),
                "-I",
                str(ROOT / "platforms/sunfish"),
                str(ROOT / "tests/sunfish_wlan_net_bridge_test.c"),
                str(ROOT / "platforms/sunfish/sunfish_wlan_net_bridge.c"),
                "-o",
                str(binary),
            ],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(binary)], cwd=ROOT, check=True)

    print("Sunfish QCACLD-to-sDDF copy bridge tests passed against pinned sDDF.")


if __name__ == "__main__":
    main()
