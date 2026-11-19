#!/usr/bin/env python3
"""Boot the actual comparison kernel/initramfs on QEMU virt, without a radio."""

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache/sunfish-mobian-comparison"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--accel", choices=["hvf", "tcg"], default="hvf" if sys.platform == "darwin" else "tcg")
    parser.add_argument("--timeout", type=int, default=120)
    args = parser.parse_args()
    qemu = shutil.which("qemu-system-aarch64")
    if not qemu:
        raise SystemExit("qemu-system-aarch64 is required")
    kernel = CACHE / "artifacts/Image"
    initrd = CACHE / "artifacts/comparison-stage0.cpio.xz"
    if not kernel.is_file() or not initrd.is_file():
        raise SystemExit("Build the reference kernel and staging userspace first")
    log = CACHE / f"qemu-stage0-{args.accel}.log"
    command = [
        qemu, "-machine", "virt,gic-version=3", "-accel", args.accel,
        "-cpu", "host" if args.accel == "hvf" else "cortex-a72",
        "-m", "2048", "-smp", "2", "-display", "none", "-monitor", "none",
        "-serial", "stdio", "-nic", "none",
        "-object", "rng-random,id=rng0,filename=/dev/urandom",
        "-device", "virtio-rng-device,rng=rng0",
        "-kernel", str(kernel), "-initrd", str(initrd),
        "-append", "console=ttyAMA0 rdinit=/init loglevel=4 panic=-1",
    ]
    required = [
        "SUNFISH COMPARISON STAGE0:",
        "STAGE0 CHECK: reference radio services masked; EFS policy unselected",
        "SUNFISH COMPARISON STAGE0 READY:",
    ]
    passed = False
    with log.open("w") as output:
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + args.timeout
            while time.monotonic() < deadline:
                contents = log.read_text(errors="replace")
                if all(marker in contents for marker in required):
                    passed = True
                    break
                if process.poll() is not None or "Kernel panic" in contents:
                    break
                time.sleep(0.25)
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
    if not passed:
        print("\n".join(log.read_text(errors="replace").splitlines()[-70:]))
        raise SystemExit(f"FAIL: staging boot did not reach its checks; see {log}")
    print(f"PASS: real comparison kernel/initramfs reached systemd and captured initial state ({args.accel})")
    print("PASS: reference radio services were masked; EFS mode remained unselected")
    print("LIMIT: QEMU virt has no Sunfish modem, WLAN, SMMU or vendor secure firmware")
    print(f"LOG: {log}")


if __name__ == "__main__":
    main()
