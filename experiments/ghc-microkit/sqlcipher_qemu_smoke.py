#!/usr/bin/env python3
"""Run the deterministic SQLCipher/VFS target fixture in QEMU."""

import os
from pathlib import Path
import selectors
import subprocess
import sys
import time


def main() -> int:
    image = Path(sys.argv[1]).resolve()
    qemu = sys.argv[2] if len(sys.argv) > 2 else "qemu-system-aarch64"
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
            if b"SQLCIPHER_QEMU|PASS|encrypted header, wrong-key rejection, reopen" in chunks:
                break
            fail_start = chunks.find(b"SQLCIPHER_QEMU|FAIL")
            if fail_start >= 0 and b"\n" in chunks[fail_start:]:
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
    log = image.parent / "sqlcipher-qemu-smoke.log"
    log.write_text(output)
    for line in output.splitlines():
        if "SQLCIPHER_QEMU|" in line:
            print(line)
    expected = (
        "SQLCIPHER_QEMU|ENTROPY_IPC_OK",
        "SQLCIPHER_QEMU|CIPHERTEXT_HEADER_OK",
        "SQLCIPHER_QEMU|WRONG_KEY_REJECTED",
        "SQLCIPHER_QEMU|PASS|encrypted header, wrong-key rejection, reopen",
    )
    positions = [output.find(marker) for marker in expected]
    if any(position < 0 for position in positions) or positions != sorted(positions):
        print(f"SQLCipher QEMU smoke failed; see {log}", file=sys.stderr)
        return 1
    print("SQLCipher AArch64 QEMU smoke passed (QEMU virtio RNG, RAM-only storage)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
