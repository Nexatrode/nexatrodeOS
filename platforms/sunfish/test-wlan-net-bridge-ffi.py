#!/usr/bin/env python3
"""Host-test QCACLD RX and TX adapters through the pinned sDDF C bridge."""

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
    archiver = shutil.which("ar")
    rustc = shutil.which("rustc")
    if compiler is None or archiver is None or rustc is None:
        raise SystemExit("`cc`, `ar`, and `rustc` are required")
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

    with tempfile.TemporaryDirectory(prefix="sunfish-wlan-net-ffi-") as temporary:
        temporary = Path(temporary)
        objects = []
        for source in (
            ROOT / "platforms/sunfish/sunfish_wlan_net_bridge.c",
            ROOT / "tests/sunfish_wlan_net_bridge_ffi_fixture.c",
        ):
            obj = temporary / f"{source.stem}.o"
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
                    "-c",
                    str(source),
                    "-o",
                    str(obj),
                ],
                cwd=ROOT,
                check=True,
            )
            objects.append(str(obj))

        library = temporary / "libsunfish_wlan_net_bridge_ffi.a"
        subprocess.run([archiver, "rcs", str(library), *objects], check=True)
        binary = temporary / "sunfish_wlan_net_bridge_ffi_test"
        subprocess.run(
            [
                rustc,
                "--edition=2024",
                "--test",
                str(ROOT / "tests/sunfish_wlan_net_bridge_ffi_test.rs"),
                "-L",
                f"native={temporary}",
                "-l",
                "static=sunfish_wlan_net_bridge_ffi",
                "-o",
                str(binary),
            ],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(binary)], cwd=ROOT, check=True)

    print("QCACLD RX/TX adapters passed through Rust, the sDDF C bridge, and shared queues.")


if __name__ == "__main__":
    main()
