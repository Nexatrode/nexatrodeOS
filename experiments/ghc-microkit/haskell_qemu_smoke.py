#!/usr/bin/env python3
"""Run the native Haskell PD until its timer and entropy checks finish."""

import os
from pathlib import Path
import selectors
import subprocess
import sys
import time


def main() -> int:
    image = Path(sys.argv[1]).resolve()
    qemu = sys.argv[2] if len(sys.argv) > 2 else "qemu-system-aarch64"
    markers = (
        "hello from Haskell",
        "HASKELL_DELAY_NS|",
        "HASKELL_ENTROPY|PASS",
        "HASKELL_TCP|SKIP",
        "HASKELL_EXIT|0",
    )
    command = [
        qemu, "-machine", "virt,virtualization=on,gic-version=2", "-accel", "tcg",
        "-cpu", "cortex-a53", "-smp", "1", "-m", "2G", "-nographic",
        "-nic", "none", "-device", "virtio-rng-device,bus=virtio-mmio-bus.8",
        "-kernel", str(image), "-global", "virtio-mmio.force-legacy=false",
        "-d", "guest_errors",
    ]
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    assert process.stdout is not None
    chunks = bytearray()
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + 30
    try:
        while time.monotonic() < deadline:
            if not selector.select(timeout=0.5):
                if process.poll() is not None:
                    break
                continue
            chunk = os.read(process.stdout.fileno(), 65536)
            if not chunk:
                break
            chunks.extend(chunk)
            if b"HASKELL_EXIT|0" in chunks or b"HASKELL_ENTROPY|FAIL" in chunks:
                break
    finally:
        selector.close()
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    output = chunks.decode("utf-8", "replace").replace("\r", "\n")
    log = image.parent / "haskell-qemu-smoke.log"
    log.write_text(output)
    for line in output.splitlines():
        if any(token in line for token in ("HASKELL", "ENTROPY|")):
            print(line)
    positions = [output.find(marker) for marker in markers]
    if any(position < 0 for position in positions) or positions != sorted(positions):
        print(f"Haskell timer/entropy smoke failed; see {log}", file=sys.stderr)
        return 1
    print("Haskell timer/entropy smoke passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
