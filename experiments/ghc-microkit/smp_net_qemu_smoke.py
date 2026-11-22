#!/usr/bin/env python3
"""Boot a guest TLS client against an isolated upstream SMP test server."""

import os
from pathlib import Path
import selectors
import socket
import subprocess
import sys
import time


def wait_for_server(process: subprocess.Popen, port: int) -> None:
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("upstream SMP server exited before listening")
        with socket.socket() as probe:
            if probe.connect_ex(("127.0.0.1", port)) == 0:
                return
        time.sleep(0.1)
    raise RuntimeError("upstream SMP server did not start")


def main() -> int:
    image = Path(sys.argv[1]).resolve()
    server_bin = Path(sys.argv[2]).resolve()
    fixture = Path(sys.argv[3]).resolve()
    qemu = sys.argv[4] if len(sys.argv) > 4 else "qemu-system-aarch64"
    env = os.environ.copy()
    env["SMP_SERVER_CFG_PATH"] = str(fixture / "etc")
    env["SMP_SERVER_LOG_PATH"] = str(fixture / "var")
    log = image.parent / "smp-server.log"
    with socket.socket() as probe:
        if probe.connect_ex(("127.0.0.1", 65224)) == 0:
            raise RuntimeError("SMP fixture port 65224 is already in use")
    chunks = bytearray()
    dns_stub = subprocess.Popen(
        [sys.executable, str(Path(__file__).resolve().parents[2] / "tests/net/dns_stub.py"),
         "--listen", "53535", "--count", "1"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    with log.open("wb") as server_log:
        server = subprocess.Popen([str(server_bin), "start"], env=env,
                                  stdout=server_log, stderr=subprocess.STDOUT)
        qemu_process = None
        try:
            wait_for_server(server, 65224)
            command = [
                qemu, "-machine", "virt,virtualization=on,gic-version=2", "-accel", "tcg",
                "-cpu", "cortex-a53", "-smp", "1", "-m", "2G", "-nographic",
                "-device", "virtio-net-device,netdev=net0,bus=virtio-mmio-bus.0",
                "-netdev", "user,id=net0",
                "-device", "virtio-rng-device,bus=virtio-mmio-bus.8",
                "-kernel", str(image), "-global", "virtio-mmio.force-legacy=false",
                "-d", "guest_errors",
            ]
            qemu_process = subprocess.Popen(
                command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            assert qemu_process.stdout is not None
            with selectors.DefaultSelector() as selector:
                selector.register(qemu_process.stdout, selectors.EVENT_READ)
                deadline = time.monotonic() + 90
                while time.monotonic() < deadline:
                    if not selector.select(timeout=0.5):
                        if qemu_process.poll() is not None:
                            break
                        continue
                    chunk = os.read(qemu_process.stdout.fileno(), 65536)
                    if not chunk:
                        break
                    chunks.extend(chunk)
                    if b"HASKELL_EXIT|0" in chunks:
                        break
        finally:
            if qemu_process is not None:
                qemu_process.terminate()
                try:
                    qemu_process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    qemu_process.kill()
                    qemu_process.wait()
            server.terminate()
            try:
                server.wait(timeout=3)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
            try:
                dns_stub.wait(timeout=1)
            except subprocess.TimeoutExpired:
                dns_stub.terminate()
                dns_stub.wait(timeout=2)
    output = chunks.decode("utf-8", "replace").replace("\r", "\n")
    guest_log = image.parent / "smp-net-qemu-smoke.log"
    guest_log.write_text(output)
    for line in output.splitlines():
        if "SMP_" in line or "HASKELL_EXIT|" in line:
            print(line)
    expected = ("SMP_DNS|PASS", "SMP_CONNECT|PASS", "SMP_TLS_PIN|PASS",
                "SMP_UPSTREAM_ENCODING|PASS", "SMP_BLOCK|PASS|",
                "SMP_VERSION_RANGE|", "SMP_SESSION|PASS",
                "SMP_CLIENT_HANDSHAKE|SENT", "SMP_PING|SENT", "SMP_PONG|PASS",
                "SMP_NEW|SENT", "SMP_IDS|PASS", "SMP_SEND|SENT",
                "SMP_SEND_OK|PASS", "SMP_GET|SENT", "SMP_MSG|PASS",
                "SMP_WRONG_PIN|REJECT",
                "HASKELL_EXIT|0")
    positions = [output.find(marker) for marker in expected]
    if (dns_stub.returncode != 0 or any(p < 0 for p in positions) or positions != sorted(positions) or
            "SMP_PROBE|FAIL" in output or "SMP_IDS|FAIL" in output or
            "SMP_NEW|KEY_FAIL" in output or "SMP_SEND_OK|FAIL" in output or
            "SMP_MSG|FAIL" in output or "SMP_WRONG_PIN|OTHER_FAIL" in output or
            "SMP_WRONG_PIN|ACCEPTED" in output):
        print(f"Upstream SMP first-block test failed; see {guest_log} and {log}",
              file=sys.stderr)
        return 1
    print("Guest DNS, pinned SMP TLS (wrong pin rejected), PING/PONG, signed queue creation, opaque SEND, and encrypted MSG delivery passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
