#!/usr/bin/env python3
"""Exercise Slint Messages against the upstream Agent in disposable QEMU."""

from __future__ import annotations

import argparse
import json
import os
import pty
import re
import select
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

from simplex_cli_interop_qemu_smoke import SocksHandler, SocksServer, run_cli


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


def run_cli_while_draining_guest(master: int, qemu: subprocess.Popen[bytes],
                                 output: bytearray, *cli_args) -> str:
    """Run the host CLI without letting verbose guest UART output fill its PTY."""
    outcome: list[tuple[bool, object]] = []

    def invoke() -> None:
        try:
            outcome.append((True, run_cli(*cli_args)))
        except BaseException as error:
            outcome.append((False, error))

    worker = threading.Thread(target=invoke, daemon=True)
    worker.start()
    while worker.is_alive():
        ready, _, _ = select.select([master], [], [], 0.25)
        if ready:
            try:
                output.extend(os.read(master, 65536))
            except OSError:
                break
        if qemu.poll() is not None:
            break
    worker.join()
    if not outcome:
        raise RuntimeError("host SimpleX CLI did not finish while QEMU was running")
    success, value = outcome[0]
    if not success:
        raise value  # type: ignore[misc]
    return value  # type: ignore[return-value]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--agent-image", type=Path, required=True)
    parser.add_argument("--smp-server", type=Path, required=True)
    parser.add_argument("--smp-fixture", type=Path, required=True)
    parser.add_argument("--dns-stub", type=Path, required=True)
    parser.add_argument("--pristine-disk", type=Path, required=True)
    parser.add_argument("--qemu", default="qemu-system-aarch64")
    parser.add_argument("--startup-timeout", type=float, default=240.0)
    parser.add_argument("--slint-ui", action="store_true",
                        help="boot the merged VirtIO GPU/input Slint shell too")
    parser.add_argument("--expect-agent-self-join", action="store_true",
                        help="require the QEMU-only Messages self-join after UI invitation creation")
    parser.add_argument("--send-ui-message", action="store_true",
                        help="type and send a test message from the Slint chat to the Agent")
    parser.add_argument("--external-cli", action="store_true",
                        help="join the UI invitation with a separate upstream SimpleX CLI")
    parser.add_argument("--simplex-chat", type=Path,
                        help="upstream SimpleX CLI binary for --external-cli")
    args = parser.parse_args()
    if args.external_cli and (not args.slint_ui or args.simplex_chat is None):
        parser.error("--external-cli requires --slint-ui and --simplex-chat")
    if args.external_cli and args.expect_agent_self_join:
        parser.error("--external-cli and --expect-agent-self-join are mutually exclusive")
    if args.send_ui_message and not (
        args.slint_ui and (args.expect_agent_self_join or args.external_cli)
    ):
        parser.error("--send-ui-message requires --slint-ui with a peer test mode")

    image = args.agent_image.resolve()
    if args.slint_ui:
        system_text = (image / "system.system").read_text()
        if '<protection_domain name="slint_ui"' not in system_text:
            raise SystemExit(
                "--slint-ui requires a merged image with a slint_ui protection domain"
            )
    fixture = args.smp_fixture.resolve()
    disk = image / "agent-service-disk.img"
    shutil.copy2(args.pristine_disk.resolve(), disk)

    env = os.environ.copy()
    env["SMP_SERVER_CFG_PATH"] = str(fixture / "etc")
    env["SMP_SERVER_LOG_PATH"] = str(fixture / "var")
    smp_log = image / "smp-server.log"
    dns_log = image / "dns-stub.log"
    with socket.socket() as probe:
        if probe.connect_ex(("127.0.0.1", 65224)) == 0:
            raise SystemExit("QEMU SimpleX test server port 65224 is already in use")
    server = subprocess.Popen(
        [str(args.smp_server.resolve()), "start"], env=env,
        stdout=smp_log.open("wb"), stderr=subprocess.STDOUT,
    )
    dns = None
    qemu = None
    master = slave = None
    qmp_path = None
    socks = None
    output = bytearray()
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
            stdout=dns_log.open("wb"), stderr=subprocess.STDOUT,
        )
        command = [
            args.qemu, "-machine", "virt,virtualization=on", "-accel", "tcg",
            "-cpu", "cortex-a53", "-smp", "1", "-m", "2G",
            "-serial", "mon:stdio", "-global", "virtio-mmio.force-legacy=false",
            "-device", f"loader,file={image / 'loader.img'},addr=0x70000000,cpu-num=0",
            "-device", "virtio-blk-device,drive=hd,bus=virtio-mmio-bus.1",
            "-device", "virtio-rng-device,bus=virtio-mmio-bus.8",
            "-device", "virtio-net-device,netdev=net0,bus=virtio-mmio-bus.0",
            "-netdev", "user,id=net0", "-nographic", "-d", "guest_errors",
            "-drive", f"file={disk},if=none,format=raw,id=hd",
        ]
        qmp_path = Path(f"/tmp/nx-agent-{os.getpid()}.sock")
        if args.slint_ui:
            if qmp_path.exists():
                qmp_path.unlink()
            command[command.index("-nographic"):command.index("-nographic") + 1] = [
                "-device",
                "virtio-gpu-device,bus=virtio-mmio-bus.31,edid=off,blob=off,max_outputs=1,indirect_desc=off,event_idx=off",
                "-device",
                "virtio-keyboard-device,bus=virtio-mmio-bus.30,indirect_desc=off,event_idx=off",
                "-device",
                "virtio-tablet-device,bus=virtio-mmio-bus.29,indirect_desc=off,event_idx=off",
                "-nographic",
            ]
            command.extend(["-qmp", f"unix:{qmp_path},server=on,wait=off"])
        master, slave = pty.openpty()
        qemu = subprocess.Popen(
            command, stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
        )
        os.close(slave)
        slave = None
        ready_deadline = time.monotonic() + args.startup_timeout
        ready_marker = b"AGENT_RUNTIME|SERVICE_READY"
        while time.monotonic() < ready_deadline:
            ready, _, _ = select.select([master], [], [], 0.25)
            if ready:
                try:
                    output.extend(os.read(master, 65536))
                except OSError:
                    break
            terminal_markers = (
                b"HASKELL_ABORT", b"HASKELL_EXIT", b"AGENT_RUNTIME|STORE_FAIL",
                b"AGENT_RUNTIME|START_FAIL",
            )
            if any(marker in output for marker in terminal_markers) or qemu.poll() is not None:
                break
            if ready_marker in output:
                break
        if ready_marker not in output:
            (image / "qemu-service.log").write_bytes(
                bytes(output).replace(b"\r", b"")
            )
            raise SystemExit(
                "Agent service did not reach ready marker; see "
                f"{image / 'qemu-service.log'}\n"
                + bytes(output).decode("utf-8", "replace")[-6000:]
            )

        # Keep the actual QEMU guest alive long enough to prove Main did not
        # fall through to HASKELL_EXIT after Agent initialization.
        # The guest's DHCP/DNS and protected network PDs can become ready after
        # the Agent service-ready marker; allow the Slint test a longer settle.
        alive_until = time.monotonic() + (30 if args.slint_ui else 8)
        while time.monotonic() < alive_until and qemu.poll() is None:
            ready, _, _ = select.select([master], [], [], 0.25)
            if ready:
                try:
                    output.extend(os.read(master, 65536))
                except OSError:
                    break
        if args.slint_ui and qemu.poll() is None:
            if b"UTM FRAME READY 300x400 STRIDE 900" not in output:
                (image / "qemu-service.log").write_bytes(
                    bytes(output).replace(b"\r", b"")
                )
                raise SystemExit("Slint did not produce its first frame; see qemu-service.log")
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                qmp_socket.settimeout(5)
                qmp_socket.connect(str(qmp_path))
                with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                    greeting = json.loads(qmp_stream.readline())
                    if "QMP" not in greeting:
                        raise RuntimeError("QEMU returned an invalid QMP greeting")
                    qmp_execute(qmp_stream, "qmp_capabilities")
                    qmp_tap(qmp_stream, 80, 118)  # Messages tile on Home.
            refresh_until = time.monotonic() + 4
            while time.monotonic() < refresh_until and qemu.poll() is None:
                ready, _, _ = select.select([master], [], [], 0.25)
                if ready:
                    try:
                        output.extend(os.read(master, 65536))
                    except OSError:
                        break
            # The 414x736 layout uses a 0.543 scale factor on the 300x400 panel.
            # NEW LINK sits at logical x=parent.width-214, y=113.
            # Opening it exercises the Slint callback and the Messages-owned URI
            # read endpoint; serial output includes only the URI length.
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                qmp_socket.settimeout(5)
                qmp_socket.connect(str(qmp_path))
                with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                    greeting = json.loads(qmp_stream.readline())
                    if "QMP" not in greeting:
                        raise RuntimeError("QEMU returned an invalid QMP greeting")
                    qmp_execute(qmp_stream, "qmp_capabilities")
                    qmp_tap(qmp_stream, 185, 63)
            invitation_until = time.monotonic() + 4
            while time.monotonic() < invitation_until and qemu.poll() is None:
                ready, _, _ = select.select([master], [], [], 0.25)
                if ready:
                    try:
                        output.extend(os.read(master, 65536))
                    except OSError:
                        break
            invitation_marker = b"SLINT SIMPLEX INVITATION READY LEN "
            invitation_deadline = time.monotonic() + 16
            while (invitation_marker not in output and time.monotonic() < invitation_deadline
                   and qemu.poll() is None):
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                    qmp_socket.settimeout(5)
                    qmp_socket.connect(str(qmp_path))
                    with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                        greeting = json.loads(qmp_stream.readline())
                        if "QMP" not in greeting:
                            raise RuntimeError("QEMU returned an invalid QMP greeting")
                        qmp_execute(qmp_stream, "qmp_capabilities")
                        qmp_tap(qmp_stream, 185, 63)  # CHECK after the async create request.
                check_until = time.monotonic() + 3
                while time.monotonic() < check_until and qemu.poll() is None:
                    ready, _, _ = select.select([master], [], [], 0.25)
                    if ready:
                        try:
                            output.extend(os.read(master, 65536))
                        except OSError:
                            break
            if (b"SIMPLEXIPC|MESSAGES_APP|CREATE_INVITATION_FAIL" in output
                    and invitation_marker not in output):
                (image / "qemu-service.log").write_bytes(
                    bytes(output).replace(b"\r", b"")
                )
                raise SystemExit(
                    "Agent could not create the Slint invitation; see qemu-service.log, "
                    "dns-stub.log, and smp-server.log"
                )
            if invitation_marker in output:
                if args.external_cli:
                    invite_prefix = b"SIMPLEX_UI_EXTERNAL_INVITE_CHUNK|"
                    candidates = {}
                    for match in re.finditer(
                        rb"SIMPLEX_UI_EXTERNAL_INVITE_CHUNK\|([0-9]+)\|([0-9a-f]+)",
                        output,
                    ):
                        try:
                            candidates.setdefault(int(match.group(1)), []).append(
                                bytes.fromhex(match.group(2).decode())
                            )
                        except ValueError:
                            continue
                    expected = None
                    for line in output.splitlines():
                        prefix = b"SLINT SIMPLEX INVITATION READY LEN "
                        if prefix in line:
                            try:
                                expected = int(line.split(prefix, 1)[1].split()[0])
                            except (ValueError, IndexError):
                                pass
                    if not candidates or expected is None:
                        raise SystemExit("test Messages PD did not export a complete QEMU-only invitation")
                    chunks = {}
                    for index in range((expected + 15) // 16):
                        chunk_length = min(16, expected - index * 16)
                        valid = [chunk for chunk in candidates.get(index, ())
                                 if len(chunk) == chunk_length]
                        if valid:
                            chunks[index] = valid[0]
                    if len(chunks) != (expected + 15) // 16:
                        raise SystemExit(
                            "QEMU serial interleaved invitation chunks; captured sizes: "
                            + repr({index: [len(chunk) for chunk in values]
                                    for index, values in candidates.items()})
                        )
                    invite_line = b"".join(chunks[index] for index in range(len(chunks)))
                    if expected is not None and len(invite_line) != expected:
                        raise SystemExit(
                            f"test invitation export length mismatch: UI={expected}, serial={len(invite_line)}"
                        )
                    invitation = invite_line.decode()
                    if not invitation.startswith("simplex:/invitation#/"):
                        raise SystemExit("test Messages PD emitted a malformed external invitation")
                    socks = SocksServer(("127.0.0.1", 0), SocksHandler)
                    SocksHandler.trace_path = image / "slint-cli-socks-tunnel.log"
                    SocksHandler.trace_path.unlink(missing_ok=True)
                    threading.Thread(target=socks.serve_forever, daemon=True).start()
                    fingerprint = (fixture / "etc/fingerprint").read_text().strip()
                    server_spec = f"smp://{fingerprint}@personal-os.test:65224"
                    peer_db = image / "slint-independent-cli-peer"
                    for suffix in ("_agent.db", "_chat.db", "_agent.db-shm", "_agent.db-wal",
                                   "_chat.db-shm", "_chat.db-wal"):
                        (image / f"slint-independent-cli-peer{suffix}").unlink(missing_ok=True)
                    cli_join = run_cli_while_draining_guest(master, qemu, output,
                        args.simplex_chat.resolve(), peer_db, "QEMU Slint peer",
                        "/connect " + invitation, "all", 8, server_spec,
                        f"127.0.0.1:{socks.server_address[1]}",
                    )
                    (image / "slint-cli-join.log").write_text(cli_join)
                    if not any(phrase in cli_join.lower() for phrase in
                               ("confirmation sent", "contact is connected")):
                        raise SystemExit("independent CLI did not confirm the Slint invitation; see slint-cli-join.log")
                # Return to Messages and verify the post-create Agent listing
                # reaches the service-backed Slint text, not only IPC logs.
                for x, y, wait_seconds in (
                    (245, 63, 1),   # HOME from the invitation view.
                    (80, 118, 2),   # Messages tile.
                    (80, 130, 2),   # Refresh the Messages connection snapshot.
                ):
                    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                        qmp_socket.settimeout(5)
                        qmp_socket.connect(str(qmp_path))
                        with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                            greeting = json.loads(qmp_stream.readline())
                            if "QMP" not in greeting:
                                raise RuntimeError("QEMU returned an invalid QMP greeting")
                            qmp_execute(qmp_stream, "qmp_capabilities")
                            qmp_tap(qmp_stream, x, y)
                    refresh_until = time.monotonic() + wait_seconds
                    while time.monotonic() < refresh_until and qemu.poll() is None:
                        ready, _, _ = select.select([master], [], [], 0.25)
                        if ready:
                            try:
                                output.extend(os.read(master, 65536))
                            except OSError:
                                break
                if args.send_ui_message:
                    # Open the actual Agent-backed connection, focus the
                    # compose field, and use QEMU's virtual keyboard path.
                    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                        qmp_socket.settimeout(5)
                        qmp_socket.connect(str(qmp_path))
                        with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                            greeting = json.loads(qmp_stream.readline())
                            if "QMP" not in greeting:
                                raise RuntimeError("QEMU returned an invalid QMP greeting")
                            qmp_execute(qmp_stream, "qmp_capabilities")
                            qmp_tap(qmp_stream, 110, 210)  # OPEN FIRST SIMPLEX CHAT.
                            qmp_tap(qmp_stream, 100, 222)  # Focus the compose field.
                            touch_keys = {
                                # QEMU's physical framebuffer is 300x400. These
                                # centers come from the rendered touch keyboard,
                                # whose logical 414x736 layout is non-uniformly
                                # scaled to that panel.
                                "h": (177, 300), "i": (218, 276), " ": (56, 348),
                                "f": (118, 300), "r": (106, 276), "o": (258, 276),
                                "m": (253, 324), "s": (62, 300), "l": (258, 300),
                                "n": (219, 324), "t": (134, 276),
                            }
                            for key in "hi from slint":
                                qmp_tap(qmp_stream, *touch_keys[key])
                                time.sleep(0.12)
                            qmp_execute(qmp_stream, "screendump", {
                                "filename": str(image / "slint-message-composer.ppm"),
                            })
                    type_until = time.monotonic() + 1
                    while time.monotonic() < type_until and qemu.poll() is None:
                        ready, _, _ = select.select([master], [], [], 0.25)
                        if ready:
                            try:
                                output.extend(os.read(master, 65536))
                            except OSError:
                                break
                    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                        qmp_socket.settimeout(5)
                        qmp_socket.connect(str(qmp_path))
                        with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                            greeting = json.loads(qmp_stream.readline())
                            if "QMP" not in greeting:
                                raise RuntimeError("QEMU returned an invalid QMP greeting")
                            qmp_execute(qmp_stream, "qmp_capabilities")
                            qmp_tap(qmp_stream, 266, 227)  # SEND.
                    send_deadline = time.monotonic() + 20
                    while (b"SIMPLEXIPC|MESSAGES_APP|SEND_MESSAGE_PASS" not in output
                           and time.monotonic() < send_deadline and qemu.poll() is None):
                        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                            qmp_socket.settimeout(5)
                            qmp_socket.connect(str(qmp_path))
                            with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                                greeting = json.loads(qmp_stream.readline())
                                if "QMP" not in greeting:
                                    raise RuntimeError("QEMU returned an invalid QMP greeting")
                                qmp_execute(qmp_stream, "qmp_capabilities")
                                qmp_tap(qmp_stream, 266, 227)  # CHECK send status.
                        check_until = time.monotonic() + 1
                        while time.monotonic() < check_until and qemu.poll() is None:
                            ready, _, _ = select.select([master], [], [], 0.25)
                            if ready:
                                try:
                                    output.extend(os.read(master, 65536))
                                except OSError:
                                    break
                    if b"SIMPLEXIPC|MESSAGES_APP|SEND_MESSAGE_PASS" not in output:
                        failure_log = re.sub(
                            rb"SIMPLEX_UI_EXTERNAL_INVITE_CHUNK[^\r\n]*",
                            b"SIMPLEX_UI_EXTERNAL_INVITE_CHUNK|<redacted>", bytes(output),
                        )
                        (image / "qemu-service.log").write_bytes(
                            failure_log.replace(b"\r", b"")
                        )
                        raise SystemExit("Slint compose did not complete an Agent send; see qemu-service.log")
                    if args.external_cli:
                        cli_inbox = run_cli_while_draining_guest(master, qemu, output,
                            args.simplex_chat.resolve(), peer_db, "QEMU Slint peer",
                            "/contacts", "all", 12, server_spec,
                            f"127.0.0.1:{socks.server_address[1]}",
                        )
                        (image / "slint-cli-inbox.log").write_text(cli_inbox)
                        if "HI FROM SLINT" not in cli_inbox:
                            raise SystemExit("independent CLI did not receive the Slint message; see slint-cli-inbox.log")
                        # Start the UI's Agent-backed long poll before the CLI
                        # sends its reply. This keeps the request live while
                        # the independent peer delivers the inbound message.
                        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                            qmp_socket.settimeout(5)
                            qmp_socket.connect(str(qmp_path))
                            with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                                greeting = json.loads(qmp_stream.readline())
                                if "QMP" not in greeting:
                                    raise RuntimeError("QEMU returned an invalid QMP greeting")
                                qmp_execute(qmp_stream, "qmp_capabilities")
                                qmp_tap(qmp_stream, 100, 200)  # Begin incoming-message long poll.
                        cli_reply = run_cli_while_draining_guest(master, qemu, output,
                            args.simplex_chat.resolve(), peer_db, "QEMU Slint peer",
                            "@NexatrodeOS REPLY FROM SIMPLEX CLI", "all", 15,
                            server_spec, f"127.0.0.1:{socks.server_address[1]}",
                        )
                        (image / "slint-cli-reply.log").write_text(cli_reply)
                        if "REPLY FROM SIMPLEX CLI" not in cli_reply:
                            raise SystemExit("independent CLI did not accept its reply; see slint-cli-reply.log")
                # The Agent-backed inbox control sits below the read-only
                # connection snapshot. Tap once to start ReceiveNext, then
                # again to read the asynchronous NoEvent response.
                receive_tap = (100, 200) if args.send_ui_message else (150, 245)
                receive_taps = 1 if args.external_cli and args.send_ui_message else 2
                for _ in range(receive_taps):
                    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                        qmp_socket.settimeout(5)
                        qmp_socket.connect(str(qmp_path))
                        with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                            greeting = json.loads(qmp_stream.readline())
                            if "QMP" not in greeting:
                                raise RuntimeError("QEMU returned an invalid QMP greeting")
                            qmp_execute(qmp_stream, "qmp_capabilities")
                            qmp_tap(qmp_stream, *receive_tap)  # CHECK INCOMING / poll status.
                    poll_until = time.monotonic() + 2
                    while time.monotonic() < poll_until and qemu.poll() is None:
                        ready, _, _ = select.select([master], [], [], 0.25)
                        if ready:
                            try:
                                output.extend(os.read(master, 65536))
                            except OSError:
                                break
                if args.send_ui_message:
                    # The self-joined peer should return the text through the
                    # same inbox control. Acknowledge it and then read back the
                    # asynchronous Agent result from the UI.
                    for _ in range(2):
                        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as qmp_socket:
                            qmp_socket.settimeout(5)
                            qmp_socket.connect(str(qmp_path))
                            with qmp_socket.makefile("rwb", buffering=0) as qmp_stream:
                                greeting = json.loads(qmp_stream.readline())
                                if "QMP" not in greeting:
                                    raise RuntimeError("QEMU returned an invalid QMP greeting")
                                qmp_execute(qmp_stream, "qmp_capabilities")
                                qmp_tap(qmp_stream, 100, 200)  # ACK / CHECK ACK STATUS.
                        ack_until = time.monotonic() + 2
                        while time.monotonic() < ack_until and qemu.poll() is None:
                            ready, _, _ = select.select([master], [], [], 0.25)
                            if ready:
                                try:
                                    output.extend(os.read(master, 65536))
                                except OSError:
                                    break
        result = bytes(output).replace(b"\r", b"")
        safe_result = re.sub(
            rb"SIMPLEX_UI_EXTERNAL_INVITE_CHUNK[^\r\n]*",
            b"SIMPLEX_UI_EXTERNAL_INVITE_CHUNK|<redacted>", result,
        )
        (image / "qemu-service.log").write_bytes(safe_result)
        if qemu.poll() is not None or b"HASKELL_EXIT" in result or b"HASKELL_ABORT" in result:
            raise SystemExit("Agent service exited or aborted after startup; see qemu-service.log")
        required = (
            b"AGENT_RUNTIME|SERVICE_READY",
            # The Agent and Messages PDs can write to the QEMU serial console
            # at the same time, so their debug lines may interleave. The
            # Messages-side result proves the IPC reply was decoded and
            # surfaced to its caller without depending on the Agent's line.
            b"SIMPLEXIPC|MESSAGES_APP|LIST_CONNECTIONS_PASS|0",
        )
        if args.slint_ui:
            required += (
                b"UTM FRAME READY 300x400 STRIDE 900",
                b"SLINT MESSAGES LOAD PASS",
                b"SLINT SIMPLEX INVITATION READY LEN ",
                # Agent and Messages share the serial console; an individual
                # first/last character may interleave. Match the unique app
                # event body rather than requiring its prefix.
                b"MESSAGES_APP|CREATE_INVITATION_PASS|HANDLE=",
            )
        if args.expect_agent_self_join:
            required += (
                b"SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_PASS|HANDLE=",
                b"SIMPLEXIPC|MESSAGES_APP|LIST_CONNECTIONS_PASS|2",
            )
        if args.external_cli:
            required += (
                b"SIMPLEX_UI_EXTERNAL_INVITE_CHUNK|",
                b"SIMPLEXIPC|MESSAGES_APP|LIST_CONNECTIONS_PASS|1",
                b"SLINT SIMPLEX EXTERNAL REPLY VERIFIED",
            )
        if args.send_ui_message:
            required += (
                b"SLINT SIMPLEX SEND REQUEST LEN 13",
                b"SIMPLEXIPC|MESSAGES_APP|SEND_MESSAGE_PASS",
                (b"SLINT SIMPLEX RECEIVE PASS LEN 22" if args.external_cli
                 else b"SLINT SIMPLEX RECEIVE PASS LEN 13"),
                b"SIMPLEXIPC|MESSAGES_APP|RECEIVE_MESSAGE_PASS",
                # The UI, Messages PD, and Agent write the same serial
                # console. Keep the authoritative IPC acknowledgement
                # marker; the adjacent Slint debug line can be interleaved.
                b"SIMPLEXIPC|MESSAGES_APP|ACKNOWLEDGE_PASS",
            )
        elif args.slint_ui:
            required += (
                b"SIMPLEXIPC|MESSAGES_APP|RECEIVE_EMPTY",
                b"SIMPLEXIPC|MESSAGES_APP|LIST_CONNECTIONS_PASS|1",
            )
        missing = [marker.decode() for marker in required if marker not in result]
        if missing:
            raise SystemExit(
                "Agent service stayed alive but its guest IPC round trip was incomplete: "
                + ", ".join(missing) + "; see qemu-service.log"
            )
        if args.slint_ui:
            if args.external_cli and args.send_ui_message:
                print("SIMPLEX AGENT SERVICE QEMU|PASS|Slint and an independent upstream CLI exchange encrypted messages in both directions through the Agent")
            elif args.expect_agent_self_join:
                if args.send_ui_message:
                    print("SIMPLEX AGENT SERVICE QEMU|PASS|Slint creates and joins a connection, then sends, receives, and acknowledges an encrypted UI message through the Agent")
                else:
                    print("SIMPLEX AGENT SERVICE QEMU|PASS|Slint invitation creates and joins an upstream SimpleX connection; Messages lists both Agent records and polls the inbox")
            else:
                print("SIMPLEX AGENT SERVICE QEMU|PASS|Slint creates/lists an upstream invitation and polls the Agent inbox")
        else:
            print("SIMPLEX AGENT SERVICE QEMU|PASS|Messages PD lists the Agent connection snapshot")
    finally:
        if socks is not None:
            socks.shutdown()
            socks.server_close()
        if qemu is not None and qemu.poll() is None:
            os.killpg(qemu.pid, signal.SIGTERM)
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(qemu.pid, signal.SIGKILL)
                qemu.wait()
        if master is not None:
            os.close(master)
        if slave is not None:
            os.close(slave)
        if qmp_path is not None and qmp_path.exists():
            qmp_path.unlink()
        if dns is not None:
            dns.terminate()
            dns.wait(timeout=3)
        server.terminate()
        try:
            server.wait(timeout=3)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()


if __name__ == "__main__":
    main()
