#!/usr/bin/env python3
"""Run the sidecar IPC packaging probe and require its protected-call marker."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--disk", required=True)
    args = parser.parse_args()

    command = [
        args.qemu,
        "-machine", "virt,virtualization=on",
        "-accel", "tcg",
        "-cpu", "cortex-a53",
        "-smp", "1",
        "-m", "2G",
        "-nographic",
        "-serial", "mon:stdio",
        "-global", "virtio-mmio.force-legacy=false",
        "-device", f"loader,file={args.image},addr=0x70000000,cpu-num=0",
        "-device", "virtio-blk-device,drive=hd,bus=virtio-mmio-bus.1",
        "-device", "virtio-rng-device,bus=virtio-mmio-bus.8",
        "-drive", f"file={args.disk},if=none,format=raw,id=hd",
        "-d", "guest_errors",
    ]
    try:
        result = subprocess.run(command, check=False, capture_output=True,
                                text=True, timeout=12)
        output = result.stdout + result.stderr
    except subprocess.TimeoutExpired as timeout:
        stdout = timeout.stdout or b""
        stderr = timeout.stderr or b""
        if isinstance(stdout, bytes):
            stdout = stdout.decode(errors="replace")
        if isinstance(stderr, bytes):
            stderr = stderr.decode(errors="replace")
        output = stdout + stderr
    (Path(args.image).parent / "qemu.log").write_text(output)

    if "SIMPLEXIPC|PASS_GUEST_PROTECTED_CALL" not in output:
        print(output)
        raise SystemExit("SimpleX sidecar QEMU protected-call smoke failed")
    if "SIMPLEXIPC|SERVICE_ROUNDTRIP" not in output:
        print(output)
        raise SystemExit("SimpleX sidecar did not process the shared request")
    if "PD 'messages_app' is now passive!" not in output:
        print(output)
        raise SystemExit("normal Messages PD did not start in the sidecar image")
    print("SimpleX sidecar QEMU protected-call smoke: PASS")


if __name__ == "__main__":
    main()
