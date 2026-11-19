#!/usr/bin/env python3
"""Boot the QEMU Messages app against the public smp4.simplex.im relay.

The guest creates a normal SimpleX invitation. This script prints that link
when it appears on the serial console so it can be pasted into the official
iOS or Android app. It does not start a local SMP server or the host CLI.
"""

from __future__ import annotations

import argparse
import json
import os
import pty
import select
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path


def qmp_execute(stream, command: str, arguments: dict | None = None) -> None:
    request = {"execute": command}
    if arguments is not None:
        request["arguments"] = arguments
    stream.write((json.dumps(request) + "\r\n").encode())
    while True:
        response = json.loads(stream.readline())
        if "error" in response:
            raise RuntimeError(f"QEMU rejected {command}: {response['error']}")
        if "return" in response:
            return


def qmp_tap(stream, x: int, y: int) -> None:
    qmp_execute(stream, "input-send-event", {"events": [
        {"type": "abs", "data": {"axis": "x", "value": (x * 32767 + 149) // 299}},
        {"type": "abs", "data": {"axis": "y", "value": (y * 32767 + 199) // 399}},
        {"type": "btn", "data": {"down": True, "button": "left"}},
        {"type": "btn", "data": {"down": False, "button": "left"}},
    ]})


def _tap(path: Path, x: int, y: int) -> None:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
        qmp_socket.settimeout(5)
        qmp_socket.connect(str(path))
        with qmp_socket.makefile("rwb", buffering=0) as stream:
            greeting = json.loads(stream.readline())
            if "QMP" not in greeting:
                raise RuntimeError("QEMU returned an invalid QMP greeting")
            qmp_execute(stream, "qmp_capabilities")
            qmp_tap(stream, x, y)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agent-image", type=Path, required=True)
    parser.add_argument("--dns-stub", type=Path, required=True)
    parser.add_argument("--pristine-disk", type=Path, required=True)
    parser.add_argument("--qemu", default="qemu-system-aarch64")
    args = parser.parse_args()

    image = args.agent_image.resolve()
    profile_dir = Path.home() / "Library/Application Support/NexatrodeOS/simplex"
    profile_dir.mkdir(parents=True, exist_ok=True)
    disk = profile_dir / "disk.img"
    session_disk = image / "public-simplex-disk.img"
    # The encrypted database is the SimpleX profile. Reuse it across boots.
    # A rebuild deletes qemu-image, so the saved copy lives outside that tree.
    if session_disk.exists() and (
        not disk.exists() or session_disk.stat().st_mtime >= disk.stat().st_mtime
    ):
        shutil.copy2(session_disk, disk)
    elif not disk.exists():
        shutil.copy2(args.pristine_disk.resolve(), disk)
    (profile_dir / "profile.txt").write_text(
        "display-name: NexatrodeOS\n"
        "database-key: qemu-agentstore-test-key-v1\n"
        "relay: smp4.simplex.im:5223\n"
        f"disk: {disk}\n"
        "The relay does not store this profile. The disk image and database key do.\n"
    )
    dns_log = image / "public-dns.log"
    dns = subprocess.Popen(
        [
            sys.executable,
            str(args.dns_stub.resolve()),
            "--listen",
            "53535",
            "--count",
            "0",
            "--forward",
        ],
        stdout=dns_log.open("wb"),
        stderr=subprocess.STDOUT,
    )
    qmp_path = image / "public-qmp.sock"
    if qmp_path.exists():
        qmp_path.unlink()
    command = [
        args.qemu,
        "-machine",
        "virt,virtualization=on",
        "-accel",
        "tcg",
        "-cpu",
        "cortex-a53",
        "-smp",
        "1",
        "-m",
        "2G",
        "-display",
        "cocoa",
        "-serial",
        "stdio",
        "-global",
        "virtio-mmio.force-legacy=false",
        "-device",
        f"loader,file={image / 'loader.img'},addr=0x70000000,cpu-num=0",
        "-device",
        "virtio-blk-device,drive=hd,bus=virtio-mmio-bus.1",
        "-device",
        "virtio-rng-device,bus=virtio-mmio-bus.8",
        "-device",
        "virtio-net-device,netdev=net0,bus=virtio-mmio-bus.0",
        "-netdev",
        "user,id=net0",
        "-drive",
        f"file={disk},if=none,format=raw,id=hd",
        "-device",
        "virtio-gpu-device,bus=virtio-mmio-bus.31,edid=off,blob=off,max_outputs=1,indirect_desc=off,event_idx=off",
        "-device",
        "virtio-keyboard-device,bus=virtio-mmio-bus.30,indirect_desc=off,event_idx=off",
        "-device",
        "virtio-tablet-device,bus=virtio-mmio-bus.29,indirect_desc=off,event_idx=off",
        "-qmp",
        f"unix:{qmp_path},server=on,wait=off",
    ]
    print(
        "QEMU Messages is starting. It will open the saved chat.",
        flush=True,
    )
    serial_log = (image / "public-serial.log").open("wb")
    output = bytearray()
    tapped_messages = False
    refreshed_list = False
    opened_chat = False
    checked_receive = False
    refresh_at = 0.0
    open_at = 0.0
    check_at = 0.0
    master, slave = pty.openpty()
    qemu = subprocess.Popen(
        command, stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
    )
    os.close(slave)
    pending = b""
    try:
        while qemu.poll() is None:
            ready, _, _ = select.select([master], [], [], 0.25)
            if ready:
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    break
                serial_log.write(chunk)
                serial_log.flush()
                output.extend(chunk.replace(b"\r", b""))
                pending += chunk.replace(b"\r", b"")
                while b"\n" in pending:
                    line, pending = pending.split(b"\n", 1)
                    text = line.decode("utf-8", "replace")
                    if (
                        "SIMPLEX_PUBLIC_INVITE|" in text
                        or "PUBLIC_SMP" in text
                        or "SERVICE_READY" in text
                        or "ALLOW_CONNECTION" in text
                        or "CONNECTION_READY" in text
                        or "START_FAIL" in text
                        or                         "CREATE_INVITATION" in text
                        or "HASKELL_ABORT" in text
                        or "RESTORE_" in text
                        or "NATIVE_TLS|" in text
                        or "HASKELL_TCP|" in text
                        or "HASKELL_DNS|" in text
                        or "LIST_CONNECTIONS" in text
                        or "CONTACT_LEN" in text
                        or "CONTACT_NAME" in text
                        or "PEER_NAME" in text
                        or "PEER_INFO" in text
                        or "SKIP_NON_TEXT" in text
                        or "RECEIVE_MESSAGE" in text
                        or "SLINT SIMPLEX" in text
                        or "GUEST_AGENT_EVENT" in text
                    ):
                        print(text, flush=True)
                    if text.startswith("SIMPLEX_PUBLIC_INVITE|"):
                        invite = text.split("|", 1)[1]
                        print(f"\nPaste this into the SimpleX app:\n{invite}\n", flush=True)
            if not tapped_messages and b"UIINPUT|READY" in output and b"RESTORE_PASS" in output:
                tapped_messages = True
                _tap(qmp_path, 80, 118)
                refresh_at = time.monotonic() + 3
            # The contact list arrives after the first paint. Refresh it, then
            # open that chat. A later tap collects the message into the thread.
            if (
                tapped_messages
                and not refreshed_list
                and refresh_at
                and time.monotonic() >= refresh_at
                and b"LIST_CONNECTIONS_PASS" in output
            ):
                refreshed_list = True
                _tap(qmp_path, 123, 150)
                open_at = time.monotonic() + 1.5
            if refreshed_list and open_at and time.monotonic() >= open_at:
                open_at = 0.0
                opened_chat = True
                _tap(qmp_path, 150, 213)
                check_at = time.monotonic() + 20
            if (
                opened_chat
                and not checked_receive
                and b"SLINT SIMPLEX RECEIVE PASS" not in output
                and (
                    b"RECEIVE_MESSAGE_PASS" in output
                    or (check_at and time.monotonic() >= check_at)
                )
            ):
                checked_receive = True
                _tap(qmp_path, 118, 200)
    except KeyboardInterrupt:
        pass
    finally:
        if qemu.poll() is None:
            os.killpg(qemu.pid, signal.SIGTERM)
            deadline = time.monotonic() + 5
            while qemu.poll() is None and time.monotonic() < deadline:
                time.sleep(0.1)
            if qemu.poll() is None:
                os.killpg(qemu.pid, signal.SIGKILL)
        dns.terminate()
        serial_log.close()
        os.close(master)


if __name__ == "__main__":
    main()
