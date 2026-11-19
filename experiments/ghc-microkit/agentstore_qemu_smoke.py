#!/usr/bin/env python3
"""Exercise the upstream AgentStore on a disposable QEMU vault disk."""

from __future__ import annotations

import argparse
import os
import pty
import select
import shutil
import signal
import subprocess
import time
from pathlib import Path


def boot(qemu: str, loader: Path, disk: Path, log: Path, marker: bytes,
         network: bool = False) -> bytes:
    command = [
        qemu,
        "-machine", "virt,virtualization=on",
        "-accel", "tcg",
        "-cpu", "cortex-a53",
        "-smp", "1",
        "-m", "2G",
        "-serial", "mon:stdio",
        "-global", "virtio-mmio.force-legacy=false",
        "-device", f"loader,file={loader},addr=0x70000000,cpu-num=0",
        "-device", "virtio-blk-device,drive=hd,bus=virtio-mmio-bus.1",
        "-device", "virtio-rng-device,bus=virtio-mmio-bus.8",
        "-nographic",
        "-d", "guest_errors",
        "-drive", f"file={disk},if=none,format=raw,id=hd",
    ]
    if network:
        command[command.index("-nographic"):command.index("-nographic")] = [
            "-device", "virtio-net-device,netdev=net0,bus=virtio-mmio-bus.0",
            "-netdev", "user,id=net0",
        ]
    master, slave = pty.openpty()
    process = subprocess.Popen(
        command, stdin=slave, stdout=slave, stderr=slave, start_new_session=True
    )
    os.close(slave)
    output = bytearray()
    # Full SimpleX Agent startup and its encrypted database migrations need
    # more than the smaller storage-only probes' original 45-second budget.
    deadline = time.monotonic() + 120
    try:
        while time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.25)
            if ready:
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                output.extend(chunk)
                current = bytes(output)
                if marker in current and b"HASKELL_EXIT" in current:
                    break
                if b"HASKELL_ABORT" in current:
                    break
            if process.poll() is not None:
                break
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
        os.close(master)

    result = bytes(output).replace(b"\r", b"")
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_bytes(result)
    if marker not in result or b"HASKELL_EXIT" not in result:
        raise SystemExit(
            f"QEMU did not reach expected marker {marker.decode()}; see {log}\n"
            + result.decode("utf-8", "replace")[-5000:]
        )
    if b"HASKELL_ABORT" in result:
        raise SystemExit(f"Haskell runtime aborted; see {log}")
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--correct-image", type=Path, required=True)
    parser.add_argument("--wrong-key-image", type=Path, required=True)
    parser.add_argument("--pristine-disk", type=Path, required=True)
    parser.add_argument("--qemu", default="qemu-system-aarch64")
    args = parser.parse_args()

    correct = args.correct_image.resolve()
    wrong = args.wrong_key_image.resolve()
    pristine = args.pristine_disk.resolve()
    disk = correct / "agentstore-disk.img"
    shutil.copy2(pristine, disk)
    pristine_bytes = pristine.read_bytes()

    first = boot(
        args.qemu,
        correct / "loader.img",
        disk,
        correct / "qemu-first-boot.log",
        b"AGENTSTORE_MIGRATIONS|PASS",
    )
    record_marker = next(
        (line for line in first.splitlines() if line.startswith(b"AGENTSTORE_RECORD|PASS|")),
        None,
    )
    if record_marker is None or b"AGENTSTORE_RECORD|FAIL" in first:
        raise SystemExit("AgentStore user record was not written and read back on first boot")
    if disk.read_bytes() == pristine_bytes:
        raise SystemExit("AgentStore boot passed but left the vault disk unchanged")

    reopen = boot(
        args.qemu,
        correct / "loader.img",
        disk,
        correct / "qemu-reopen.log",
        b"AGENTSTORE_MIGRATIONS|PASS",
    )
    reopened_record_marker = next(
        (line for line in reopen.splitlines() if line.startswith(b"AGENTSTORE_RECORD|PASS|")),
        None,
    )
    if reopened_record_marker != record_marker or b"AGENTSTORE_RECORD|FAIL" in reopen:
        raise SystemExit(
            "AgentStore record did not survive same-key reopen: "
            f"first={record_marker!r}, reopen={reopened_record_marker!r}"
        )
    before_wrong_key = disk.read_bytes()
    wrong_key = boot(
        args.qemu,
        wrong / "loader.img",
        disk,
        wrong / "qemu-wrong-key.log",
        b"AGENTSTORE_WRONG_KEY|REJECTED",
    )
    if b"AGENTSTORE_WRONG_KEY|ACCEPTED" in wrong_key:
        raise SystemExit("the deliberately wrong AgentStore key was accepted")
    if disk.read_bytes() != before_wrong_key:
        raise SystemExit("wrong-key attempt modified the disposable vault disk")

    print(
        "SIMPLEX AGENTSTORE QEMU|PASS|migrations, user-record round trip, "
        "same-key reopen, wrong-key rejection"
    )


if __name__ == "__main__":
    main()
