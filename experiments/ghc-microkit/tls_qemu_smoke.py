#!/usr/bin/env python3
"""Boot the guest TLS link probe without attempting a network handshake."""

import hashlib
import os
from pathlib import Path
import selectors
import subprocess
import sys
import time


def main() -> int:
    image = Path(sys.argv[1]).resolve()
    qemu = sys.argv[2] if len(sys.argv) > 2 else "qemu-system-aarch64"
    expected = (
        "TLS_CLOCK|QEMU_RTC_READY",
        "TLS_CRYPTO|SHA256|" + hashlib.sha256(bytes([1, 2, 3])).hexdigest(),
        "TLS_BACKEND|PASS",
        "TLS_ENTROPY|PASS",
        "TLS_CONTEXT|PASS",
        "TLS_TIME|",
        "TLS_HANDSHAKE|SKIP",
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
    deadline = time.monotonic() + 45
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
            if b"HASKELL_EXIT|0" in chunks:
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
    log = image.parent / "tls-qemu-smoke.log"
    log.write_text(output)
    for line in output.splitlines():
        if any(token in line for token in ("TLS_CLOCK|", "TLS_CRYPTO|", "TLS_BACKEND|", "TLS_ENTROPY|", "TLS_CONTEXT|", "TLS_TIME|", "TLS_HANDSHAKE|", "HASKELL_EXIT|")):
            print(line)
    positions = [output.find(marker) for marker in expected]
    if any(position < 0 for position in positions) or positions != sorted(positions):
        print(f"TLS crypto/backend smoke failed; see {log}", file=sys.stderr)
        return 1
    time_lines = [line for line in output.splitlines() if line.startswith("TLS_TIME|")]
    if len(time_lines) != 1 or not time_lines[0][9:].isdigit() or \
            abs(int(time_lines[0][9:]) - time.time()) > 300:
        print(f"QEMU RTC differs from host time; see {log}", file=sys.stderr)
        return 1
    print("TLS crypto/backend/entropy/context smoke passed (no handshake attempted)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
