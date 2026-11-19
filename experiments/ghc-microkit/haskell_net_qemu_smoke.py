#!/usr/bin/env python3
"""Prove native Haskell streaming TCP and reconnect through the isolated lwIP PD."""

import os
from pathlib import Path
import selectors
import socket
import subprocess
import sys
import threading
import time

BURST_SIZE = 20 * 1024  # Exceeds SimpleXMQ's 16 KiB SMP transport block.
BURST = (b"INBOUND-BURST-" * 1500)[:BURST_SIZE]
OUTBOUND = (b"OUTBOUND-BURST-" * 1500)[:BURST_SIZE]
PAYLOADS = ((b"NATIVE-STREAM-" * 14)[:192], b"RECONNECT-OK",
            b"STALL-THEN-CANCEL", b"RECOVER-OK", b"", OUTBOUND)


def exercise_sessions(listener: socket.socket, seen: list[bytes]) -> None:
    try:
        listener.settimeout(60)
        for index, expected in enumerate(PAYLOADS):
            conn, _ = listener.accept()
            with conn:
                conn.settimeout(10)
                received = bytearray()
                while len(received) < len(expected):
                    chunk = conn.recv(len(expected) - len(received))
                    if not chunk:
                        break
                    received.extend(chunk)
                seen.append(bytes(received))
                if bytes(received) != expected:
                    return
                if index == 4:
                    conn.sendall(BURST)
                    continue
                if index == 5:
                    conn.sendall(b"OUTBOUND-OK")
                    continue
                if index == 2:
                    # Keep the stream open and silent until Haskell cancels it.
                    try:
                        conn.recv(1)
                    except ConnectionResetError:
                        pass
                    continue
                for offset in range(0, len(expected), 7):
                    conn.sendall(expected[offset:offset + 7])
    except OSError:
        pass


def main() -> int:
    image = Path(sys.argv[1]).resolve()
    qemu = sys.argv[2] if len(sys.argv) > 2 else "qemu-system-aarch64"
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 1241))
    listener.listen(1)
    seen: list[bytes] = []
    server = threading.Thread(target=exercise_sessions, args=(listener, seen), daemon=True)
    server.start()
    dns_stub = subprocess.Popen(
        [sys.executable, str(Path(__file__).resolve().parents[2] / "tests/net/dns_stub.py"),
         "--listen", "53535", "--count", "1"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    command = [
        qemu, "-machine", "virt,virtualization=on,gic-version=2", "-accel", "tcg",
        "-cpu", "cortex-a53", "-smp", "1", "-m", "2G", "-nographic",
        "-device", "virtio-net-device,netdev=net0,bus=virtio-mmio-bus.0",
        "-netdev", "user,id=net0",
        "-device", "virtio-rng-device,bus=virtio-mmio-bus.8",
        "-kernel", str(image), "-global", "virtio-mmio.force-legacy=false",
        "-d", "guest_errors",
    ]
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    assert process.stdout is not None
    chunks = bytearray()
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + 60
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
            if b"HASKELL_EXIT|0" in chunks or b"HASKELL_TCP|FAIL" in chunks:
                break
    finally:
        selector.close()
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        listener.close()
        server.join(timeout=1)
        try:
            dns_stub.wait(timeout=1)
        except subprocess.TimeoutExpired:
            dns_stub.terminate()
            dns_stub.wait(timeout=2)
    output = chunks.decode("utf-8", "replace").replace("\r", "\n")
    log = image.parent / "haskell-net-qemu-smoke.log"
    log.write_text(output)
    for line in output.splitlines():
        if any(token in line for token in ("HASKELL", "ENTROPY|", "DHCP request")):
            print(line)
    markers = ("HASKELL_ENTROPY|PASS", "HASKELL_DNS|PASS",
               "HASKELL_TCP|STREAM_PASS",
               "HASKELL_TCP|RECONNECT_PASS", "HASKELL_TCP|TIMEOUT_PASS",
               "HASKELL_TCP|RECOVER_PASS", "HASKELL_TCP|BURST_PASS",
               "HASKELL_TCP|OUTBOUND_PASS",
               "HASKELL_TCP|PASS", "HASKELL_EXIT|0")
    positions = [output.find(marker) for marker in markers]
    if (dns_stub.returncode != 0 or seen != list(PAYLOADS) or
            any(p < 0 for p in positions) or positions != sorted(positions)):
        print(f"Native Haskell TCP smoke failed; see {log}", file=sys.stderr)
        return 1
    print("Native Haskell DNS, streaming TCP, timeout recovery, and bidirectional bursts passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
