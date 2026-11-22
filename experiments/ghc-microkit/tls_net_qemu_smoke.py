#!/usr/bin/env python3
"""QEMU-only authenticated TLS 1.3 echo over the isolated network PD."""

import os
from pathlib import Path
import selectors
import socket
import ssl
import subprocess
import sys
import threading
import time


PAYLOAD = b"TLS-PING"
FIXTURE = Path(__file__).resolve().parent / "fixtures/tls"


def serve(listener: socket.socket, outcome: list[str], expired: bool) -> None:
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_3
    context.maximum_version = ssl.TLSVersion.TLSv1_3
    context.load_cert_chain(str(FIXTURE / "server-chain.crt"), str(FIXTURE / "server.key"))
    listener.settimeout(90)
    for index in range(4):
        try:
            raw, _ = listener.accept()
            with raw:
                raw.settimeout(20)
                with context.wrap_socket(raw, server_side=True) as tls:
                    received = tls.recv(128)
                    if expired:
                        outcome.append("rejected" if not received else
                                       f"expired-certificate data escaped: {received!r}")
                    elif index in (0, 2):
                        if received != PAYLOAD:
                            outcome.append(f"wrong payload: {received!r}")
                            return
                        tls.sendall(received)
                        outcome.append("pass")
                    elif received:
                        outcome.append(f"rejected-connection data escaped: {received!r}")
                    else:
                        outcome.append("rejected")
        except (OSError, ssl.SSLError) as error:
            if expired or (index in (1, 3) and len(outcome) == index):
                outcome.append("rejected")
            else:
                outcome.append(str(error))
            if index == 0 and not expired:
                return


def main() -> int:
    image = Path(sys.argv[1]).resolve()
    qemu = sys.argv[2] if len(sys.argv) > 2 else "qemu-system-aarch64"
    expired = len(sys.argv) > 3 and sys.argv[3] == "--expired"
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 1241))
    listener.listen(4)
    outcome: list[str] = []
    server = threading.Thread(target=serve, args=(listener, outcome, expired), daemon=True)
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
    if expired:
        command.extend(["-rtc", "base=2037-01-01"])
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    assert process.stdout is not None
    chunks = bytearray()
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + 90
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
            if b"HASKELL_EXIT|0" in chunks or (not expired and b"TLS_HANDSHAKE|FAIL" in chunks):
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
    log = image.parent / ("tls-net-qemu-expired.log" if expired else
                          "tls-net-qemu-smoke.log")
    log.write_text(output)
    for line in output.splitlines():
        if any(token in line for token in ("TLS_", "SMP_", "HASKELL_EXIT|")):
            print(line)
    expected = (("TLS_CLOCK|QEMU_RTC_READY", "TLS_ENTROPY|PASS",
                 "TLS_CONTEXT|PASS", "TLS_TIME|", "TLS_HANDSHAKE|FAIL|",
                 "CertificateExpired", "SMP_PIN_EXPIRED|REJECT",
                 "SMP_WRONG_PIN|REJECT", "HASKELL_EXIT|0") if expired else
                ("TLS_CLOCK|QEMU_RTC_READY", "TLS_ENTROPY|PASS",
                 "TLS_CONTEXT|PASS", "TLS_TIME|", "TLS_HANDSHAKE|PASS",
                 "TLS_WRONG_HOST|REJECT", "SMP_PIN|PASS",
                 "SMP_WRONG_PIN|REJECT", "HASKELL_EXIT|0"))
    positions = [output.find(marker) for marker in expected]
    expected_outcome = (["rejected"] * 4 if expired else
                        ["pass", "rejected", "pass", "rejected"])
    if (dns_stub.returncode != 0 or outcome != expected_outcome or
            any(position < 0 for position in positions) or positions != sorted(positions)):
        print(f"Authenticated TLS test failed: server={outcome}; see {log}", file=sys.stderr)
        return 1
    time_lines = [line for line in output.splitlines() if line.startswith("TLS_TIME|")]
    expected_time = 2114380800 if expired else time.time()
    if len(time_lines) != 1 or not time_lines[0][9:].isdigit() or \
            abs(int(time_lines[0][9:]) - expected_time) > 300:
        print(f"QEMU RTC differs from host time; see {log}", file=sys.stderr)
        return 1
    print("Expired certificate rejected" if expired else
          "TLS 1.3 echo passed; wrong hostname and SimpleX pin rejected")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
