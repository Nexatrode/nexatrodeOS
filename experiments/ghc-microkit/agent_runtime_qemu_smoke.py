#!/usr/bin/env python3
"""Boot the upstream SimpleX agent lifecycle against the persistent QEMU vault."""

from __future__ import annotations

import argparse
import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

from agentstore_qemu_smoke import boot


def record_marker(output: bytes, phase: str) -> bytes:
    marker = next(
        (line for line in output.splitlines() if line.startswith(b"AGENTSTORE_RECORD|PASS|")),
        None,
    )
    if marker is None or b"AGENTSTORE_RECORD|FAIL" in output:
        raise SystemExit(f"AgentStore user record {phase} did not pass")
    return marker


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--agent-image", type=Path, required=True)
    parser.add_argument("--wrong-key-image", type=Path, required=True)
    parser.add_argument("--pristine-disk", type=Path, required=True)
    parser.add_argument("--smp-server", type=Path, required=True)
    parser.add_argument("--smp-fixture", type=Path, required=True)
    parser.add_argument("--dns-stub", type=Path, required=True)
    parser.add_argument("--qemu", default="qemu-system-aarch64")
    args = parser.parse_args()

    agent = args.agent_image.resolve()
    wrong = args.wrong_key_image.resolve()
    pristine = args.pristine_disk.resolve()
    disk = agent / "agent-runtime-disk.img"
    shutil.copy2(pristine, disk)

    fixture = args.smp_fixture.resolve()
    env = os.environ.copy()
    env["SMP_SERVER_CFG_PATH"] = str(fixture / "etc")
    env["SMP_SERVER_LOG_PATH"] = str(fixture / "var")
    smp_log = agent / "smp-server.log"
    with socket.socket() as probe:
        if probe.connect_ex(("127.0.0.1", 65224)) == 0:
            raise SystemExit("QEMU SimpleX test server port 65224 is already in use")
    server = subprocess.Popen(
        [str(args.smp_server.resolve()), "start"], env=env,
        stdout=smp_log.open("wb"), stderr=subprocess.STDOUT,
    )
    dns = None
    try:
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if server.poll() is not None:
                raise SystemExit(f"QEMU SimpleX test server exited; see {smp_log}")
            with socket.socket() as probe:
                if probe.connect_ex(("127.0.0.1", 65224)) == 0:
                    break
            time.sleep(0.1)
        else:
            raise SystemExit(f"QEMU SimpleX test server did not listen; see {smp_log}")

        dns = subprocess.Popen(
            [sys.executable, str(args.dns_stub.resolve()), "--listen", "53535", "--count", "0"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        first = boot(
            args.qemu, agent / "loader.img", disk,
            agent / "qemu-first-boot.log", b"AGENT_RUNTIME|DISPOSE_PASS", network=True,
        )
        first_record = record_marker(first, "first boot")
        for marker in (
            b"AGENTSTORE_MIGRATIONS|PASS", b"AGENT_RUNTIME|START_PASS",
            b"AGENT_RUNTIME|MESSAGE_TEST_PASS|encrypted-send-receive-ack",
        ):
            if marker not in first:
                diagnostics = [
                    line.decode("utf-8", "replace")
                    for line in first.splitlines()
                    if line.startswith((
                        b"AGENT_RUNTIME|", b"AGENT_EVENT|",
                        b"SIMPLEX_SQL_ERROR|",
                    ))
                ]
                raise SystemExit(
                    f"missing runtime marker: {marker.decode()}\n"
                    + "observed agent diagnostics:\n"
                    + "\n".join(diagnostics)
                )

        reopened = boot(
            args.qemu, agent / "loader.img", disk,
            agent / "qemu-reopen.log", b"AGENT_RUNTIME|DISPOSE_PASS", network=True,
        )
        reopened_record = record_marker(reopened, "reopen")
        if reopened_record != first_record:
            raise SystemExit(
                f"AgentStore row changed after reboot: {first_record!r} vs {reopened_record!r}"
            )
        if (
            b"AGENT_RUNTIME|START_PASS" not in reopened
            or b"AGENT_RUNTIME|MESSAGE_TEST_PASS|encrypted-send-receive-ack" not in reopened
        ):
            raise SystemExit("upstream agent or its SMP test failed after same-key reopen")

        before_wrong_key = disk.read_bytes()
        wrong_output = boot(
            args.qemu, wrong / "loader.img", disk,
            wrong / "qemu-runtime-wrong-key.log", b"AGENTSTORE_WRONG_KEY|REJECTED",
        )
        if b"AGENTSTORE_WRONG_KEY|ACCEPTED" in wrong_output:
            raise SystemExit("wrong AgentStore key was accepted")
        if disk.read_bytes() != before_wrong_key:
            raise SystemExit("wrong-key attempt modified the QEMU vault disk")
    finally:
        server.terminate()
        try:
            server.wait(timeout=3)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()
        if dns is not None:
            dns.terminate()
            try:
                dns.wait(timeout=2)
            except subprocess.TimeoutExpired:
                dns.kill()
                dns.wait()

    print("SIMPLEX AGENT RUNTIME QEMU|PASS|upstream encrypted invitation/message/ack round trip, persistent vault, wrong-key rejection")


if __name__ == "__main__":
    main()
