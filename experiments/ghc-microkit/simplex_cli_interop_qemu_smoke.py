#!/usr/bin/env python3
"""Exchange encrypted SimpleX messages between the QEMU guest and host CLI."""

from __future__ import annotations

import argparse
import os
import pty
import select
import shutil
import signal
import socket
import struct
import subprocess
import sys
import threading
import time
from socketserver import BaseRequestHandler, ThreadingTCPServer
from pathlib import Path


def wait_qemu(master: int, process: subprocess.Popen[bytes], output: bytearray,
              marker: bytes, timeout_s: int) -> bool:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        # A prior wait may have read several serial lines at once, including
        # this marker and the process-exit marker. Check accumulated output
        # before treating HASKELL_EXIT as a failure for this wait.
        if marker in output and marker != b"SIMPLEX_INTEROP_INVITE|":
            return True
        ready, _, _ = select.select([master], [], [], 0.2)
        if ready:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                return marker in output
            if not chunk:
                return marker in output
            output.extend(chunk)
            if marker in output:
                if marker != b"SIMPLEX_INTEROP_INVITE|":
                    return True
                start = output.index(marker) + len(marker)
                if b"\n" in output[start:]:
                    return True
            if b"HASKELL_ABORT" in output or b"HASKELL_EXIT|" in output:
                return False
        if process.poll() is not None:
            return marker in output
    return False


class SocksHandler(BaseRequestHandler):
    trace_path: Path | None = None

    def trace(self, direction: str, size: int) -> None:
        peer = self.client_address[0]
        if self.trace_path is not None:
            with self.trace_path.open("a") as log:
                log.write(f"peer={peer} direction={direction} bytes={size} time={time.monotonic():.3f}\n")

    def handle(self) -> None:
        client = self.request
        self.trace("connected", 0)
        greeting = self.recv_exact(client, 2)
        if greeting[0] != 5:
            return
        methods = self.recv_exact(client, greeting[1])
        if 0 in methods:
            client.sendall(b"\x05\x00")
        elif 2 in methods:
            client.sendall(b"\x05\x02")
            auth_version, user_len = self.recv_exact(client, 2)
            if auth_version != 1:
                return
            self.recv_exact(client, user_len)
            pass_len = self.recv_exact(client, 1)[0]
            self.recv_exact(client, pass_len)
            client.sendall(b"\x01\x00")
        else:
            client.sendall(b"\x05\xff")
            return
        header = self.recv_exact(client, 4)
        if header[0] != 5 or header[1] != 1:
            client.sendall(b"\x05\x07\x00\x01\x00\x00\x00\x00\x00\x00")
            return
        if header[3] == 1:
            self.recv_exact(client, 4)
        elif header[3] == 3:
            size = self.recv_exact(client, 1)[0]
            host = self.recv_exact(client, size).decode("ascii")
            if host != "personal-os.test":
                client.sendall(b"\x05\x04\x00\x01\x00\x00\x00\x00\x00\x00")
                return
        elif header[3] == 4:
            self.recv_exact(client, 16)
        else:
            client.sendall(b"\x05\x08\x00\x01\x00\x00\x00\x00\x00\x00")
            return
        port = struct.unpack("!H", self.recv_exact(client, 2))[0]
        if port != 65224:
            client.sendall(b"\x05\x02\x00\x01\x00\x00\x00\x00\x00\x00")
            return
        try:
            upstream = socket.create_connection(("127.0.0.1", port), timeout=5)
        except OSError:
            client.sendall(b"\x05\x05\x00\x01\x00\x00\x00\x00\x00\x00")
            return
        client.sendall(b"\x05\x00\x00\x01\x7f\x00\x00\x01\xff\xff")
        with upstream:
            while True:
                # SMP subscriptions are long-lived and may be quiet while
                # the peer starts its next CLI command. Keep the TCP tunnel
                # open until one side closes instead of treating idle as EOF.
                ready, _, _ = select.select([client, upstream], [], [], 30)
                if not ready:
                    continue
                for source in ready:
                    try:
                        data = source.recv(65536)
                    except OSError:
                        return
                    if not data:
                        return
                    target = upstream if source is client else client
                    self.trace("guest_to_smp" if source is client else "smp_to_guest", len(data))
                    target.sendall(data)

    @staticmethod
    def recv_exact(sock: socket.socket, size: int) -> bytes:
        chunks = bytearray()
        while len(chunks) < size:
            part = sock.recv(size - len(chunks))
            if not part:
                raise ConnectionError("SOCKS client disconnected during request")
            chunks.extend(part)
        return bytes(chunks)


class SocksServer(ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def run_cli(binary: Path, db_prefix: Path, display_name: str, command: str,
            log_mode: str, delay: int, server_spec: str, socks_address: str) -> str:
    result = subprocess.run(
        [
            str(binary), "-d", str(db_prefix), "-s", server_spec,
            "--user-display-name", display_name,
            "-y", "--execute-log", log_mode, "--log-level", "debug", "--log-agent",
            "--socks-proxy", socks_address, "--socks-mode", "always",
            "-e", command, "-t", str(delay),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
            timeout=120,
        check=False,
    )
    output = result.stdout
    if result.returncode:
        raise RuntimeError(f"SimpleX CLI command failed ({result.returncode}):\n{output[-4000:]}")
    return output


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--agent-image", type=Path, required=True)
    parser.add_argument("--pristine-disk", type=Path, required=True)
    parser.add_argument("--smp-server", type=Path, required=True)
    parser.add_argument("--smp-fixture", type=Path, required=True)
    parser.add_argument("--dns-stub", type=Path, required=True)
    parser.add_argument("--simplex-chat", type=Path, required=True)
    parser.add_argument("--qemu", default="qemu-system-aarch64")
    parser.add_argument("--bidirectional", action="store_true")
    args = parser.parse_args()

    agent = args.agent_image.resolve()
    disk = agent / "agent-runtime-disk.img"
    shutil.copy2(args.pristine_disk.resolve(), disk)
    fixture = args.smp_fixture.resolve()
    env = os.environ.copy()
    env["SMP_SERVER_CFG_PATH"] = str(fixture / "etc")
    env["SMP_SERVER_LOG_PATH"] = str(fixture / "var")
    smp_log = agent / "smp-server.log"

    with socket.socket() as probe:
        if probe.connect_ex(("127.0.0.1", 65224)) == 0:
            raise SystemExit("QEMU SimpleX test server port 65224 is already in use")
    server = subprocess.Popen(
        [str(args.smp_server.resolve()), "start", "--log-level", "debug"], env=env,
        stdout=smp_log.open("wb"), stderr=subprocess.STDOUT,
    )
    dns = None
    qemu = None
    master = None
    socks = SocksServer(("127.0.0.1", 0), SocksHandler)
    SocksHandler.trace_path = agent / "socks-tunnel.log"
    SocksHandler.trace_path.unlink(missing_ok=True)
    threading.Thread(target=socks.serve_forever, daemon=True).start()
    socks_address = f"127.0.0.1:{socks.server_address[1]}"
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
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        command = [
            args.qemu,
            "-machine", "virt,virtualization=on", "-accel", "tcg", "-cpu", "cortex-a53",
            "-smp", "1", "-m", "2G", "-serial", "mon:stdio",
            "-global", "virtio-mmio.force-legacy=false",
            "-device", f"loader,file={agent / 'loader.img'},addr=0x70000000,cpu-num=0",
            "-device", "virtio-blk-device,drive=hd,bus=virtio-mmio-bus.1",
            "-device", "virtio-rng-device,bus=virtio-mmio-bus.8",
            "-device", "virtio-net-device,netdev=net0,bus=virtio-mmio-bus.0",
            "-netdev", "user,id=net0", "-nographic", "-d", "guest_errors",
            "-drive", f"file={disk},if=none,format=raw,id=hd",
        ]
        master, slave = pty.openpty()
        qemu = subprocess.Popen(
            command, stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
        )
        os.close(slave)

        if not wait_qemu(master, qemu, output, b"SIMPLEX_INTEROP_INVITE|", 120):
            raise RuntimeError("QEMU did not emit the external invitation; see qemu log")
        invitation = output.split(b"SIMPLEX_INTEROP_INVITE|", 1)[1].splitlines()[0].strip().decode()
        if not invitation.startswith("simplex:/invitation#/"):
            raise RuntimeError(f"guest emitted an unexpected invite URI: {invitation[:80]!r}")

        peer_db = agent / "independent-cli-peer"
        for suffix in ("_agent.db", "_chat.db", "_agent.db-shm", "_agent.db-wal", "_chat.db-shm", "_chat.db-wal"):
            (agent / f"independent-cli-peer{suffix}").unlink(missing_ok=True)
        fingerprint = (fixture / "etc/fingerprint").read_text().strip()
        server_spec = f"smp://{fingerprint}@personal-os.test:65224"
        join_log = run_cli(
            args.simplex_chat.resolve(), peer_db, "QEMU CLI peer",
            "/connect " + invitation, "all", 8, server_spec, socks_address,
        )
        (agent / "cli-join.log").write_text(join_log)
        if args.bidirectional and not wait_qemu(
            master, qemu, output, b"SIMPLEX_INTEROP_GUEST_SEND_ACCEPTED|PASS", 45,
        ):
            diagnostics = [
                line.decode("utf-8", "replace")
                for line in bytes(output).replace(b"\r", b"").splitlines()
                if line.startswith((b"AGENT_RUNTIME|", b"AGENT_EVENT|", b"SIMPLEX_INTEROP_"))
            ]
            raise RuntimeError(
                "QEMU guest did not get its first message accepted by SMP; guest phases:\n"
                + "\n".join(diagnostics[-80:])
            )
        if "confirmation sent" not in join_log.lower() and "contact is connected" not in join_log.lower():
            raise RuntimeError(f"host CLI did not confirm joining the guest invitation:\n{join_log[-4000:]}")
        if not args.bidirectional and "hello from NexatrodeOS QEMU" not in join_log:
            raise RuntimeError(f"upstream CLI did not display the guest's message:\n{join_log[-4000:]}")

        if args.bidirectional:
            # The one-shot /connect process may exit before the incoming
            # message is delivered. Reopen the same CLI store and let startup
            # subscribe and persist the newly confirmed contact before sending
            # a reply. A send issued during that first startup can race contact
            # creation even though the incoming message is being processed.
            ready_log = run_cli(
                args.simplex_chat.resolve(), peer_db, "QEMU CLI peer",
                "/contacts", "all", 10, server_spec, socks_address,
            )
            (agent / "cli-peer-ready.log").write_text(ready_log)
            if "hello from NexatrodeOS QEMU" not in ready_log:
                raise RuntimeError(
                    "upstream CLI did not receive the guest's first message while restoring its contact:\n"
                    + ready_log[-4000:]
                )
            reply_log = run_cli(
                args.simplex_chat.resolve(), peer_db, "QEMU CLI peer",
                "@NexatrodeOS hello back from upstream SimpleX CLI", "all", 15,
                server_spec, socks_address,
            )
            (agent / "cli-reply.log").write_text(reply_log)
            if "hello back from upstream SimpleX CLI" not in reply_log:
                raise RuntimeError(f"upstream CLI did not accept its reply command:\n{reply_log[-4000:]}")

        if not wait_qemu(master, qemu, output, b"AGENT_RUNTIME|DISPOSE_PASS", 75):
            diagnostics = [
                line.decode("utf-8", "replace")
                for line in bytes(output).replace(b"\r", b"").splitlines()
                if line.startswith((b"AGENT_RUNTIME|", b"AGENT_EVENT|", b"SIMPLEX_SQL_ERROR|"))
            ]
            raise RuntimeError(
                "QEMU guest did not dispose the Agent client after the independent-client message exchange:\n"
                + "\n".join(diagnostics[-80:])
            )
        if args.bidirectional:
            if (
                b"AGENT_RUNTIME|EXTERNAL_BIDIRECTIONAL_PASS|encrypted-reply-received-acked" not in output
                or b"SIMPLEX_INTEROP_INBOUND|" not in output
                or b"hello back from upstream SimpleX CLI" not in output
                or b"SIMPLEX_INTEROP_DUPLICATE_SUBSCRIBE|PASS" not in output
            ):
                diagnostics = [
                    line.decode("utf-8", "replace")
                    for line in bytes(output).replace(b"\r", b"").splitlines()
                    if line.startswith((b"AGENT_RUNTIME|", b"AGENT_EVENT|", b"SIMPLEX_INTEROP_"))
                ]
                raise RuntimeError(
                    "upstream CLI accepted the reverse SEND, but the guest did not receive it:\n"
                    + "\n".join(diagnostics[-80:])
                )
            first_boot = bytes(output).replace(b"\r", b"")
            (agent / "qemu-cli-interop-first-boot.log").write_bytes(first_boot)
            created = next(
                (line.split(b"|", 2)[2] for line in first_boot.splitlines()
                 if line.startswith(b"AGENT_RUNTIME|CONN_CREATED|")),
                None,
            )
            if created is None:
                raise RuntimeError("first guest boot did not report its stored connection ID")

            # Restart the same Microkit image with the same encrypted disk and
            # the same upstream CLI database. The guest's existing connection
            # record selects the resume path instead of creating another invite.
            if qemu.poll() is None:
                os.killpg(qemu.pid, signal.SIGTERM)
                try:
                    qemu.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(qemu.pid, signal.SIGKILL)
                    qemu.wait()
            os.close(master)
            master, slave = pty.openpty()
            output = bytearray()
            qemu = subprocess.Popen(
                command, stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
            )
            os.close(slave)
            if not wait_qemu(master, qemu, output, b"SIMPLEX_INTEROP_RESUME_READY|PASS", 120):
                diagnostics = [
                    line.decode("utf-8", "replace")
                    for line in bytes(output).replace(b"\r", b"").splitlines()
                    if line.startswith((b"AGENT_RUNTIME|", b"AGENT_EVENT|", b"SIMPLEX_INTEROP_"))
                ]
                raise RuntimeError(
                    "guest did not restore the existing SimpleX contact after reboot:\n"
                    + "\n".join(diagnostics[-80:])
                )
            restored = next(
                (line.split(b"|", 2)[2] for line in bytes(output).splitlines()
                 if line.startswith(b"AGENT_RUNTIME|CONN_RESTORED|")),
                None,
            )
            if restored != created:
                raise RuntimeError(
                    f"guest resumed a different connection after reboot: created={created!r}, restored={restored!r}"
                )
            if not wait_qemu(
                master, qemu, output, b"SIMPLEX_INTEROP_RESUME_OUTBOUND_QUEUED|PASS|", 30,
            ):
                raise RuntimeError("restored guest did not enqueue on its persisted contact after reboot")
            resume_reply_log = run_cli(
                args.simplex_chat.resolve(), peer_db, "QEMU CLI peer",
                "@NexatrodeOS hello after guest reboot", "all", 30,
                server_spec, socks_address,
            )
            (agent / "cli-resume-reply.log").write_text(resume_reply_log)
            if "SEND <MSG>" not in resume_reply_log:
                raise RuntimeError(f"upstream CLI did not send the post-reboot reply:\n{resume_reply_log[-4000:]}")
            if "hello after NexatrodeOS reboot" not in resume_reply_log:
                raise RuntimeError(f"upstream CLI did not receive the guest's post-reboot message:\n{resume_reply_log[-4000:]}")
            if not wait_qemu(
                master, qemu, output, b"SIMPLEX_INTEROP_RESUME_INBOUND_ACK|PASS", 60,
            ):
                diagnostics = [
                    line.decode("utf-8", "replace")
                    for line in bytes(output).replace(b"\r", b"").splitlines()
                    if line.startswith((b"AGENT_RUNTIME|", b"AGENT_EVENT|", b"SIMPLEX_INTEROP_"))
                ]
                raise RuntimeError(
                    "guest did not receive and acknowledge the post-reboot CLI reply:\n"
                    + "\n".join(diagnostics[-80:])
                )
            if b"hello after guest reboot" not in output:
                raise RuntimeError("guest reported post-reboot inbound success without the expected message body")
            (agent / "cli-resume.log").write_text(resume_reply_log)
            if not wait_qemu(master, qemu, output, b"AGENT_RUNTIME|DISPOSE_PASS", 30):
                raise RuntimeError("guest did not cleanly dispose after the post-reboot send")
            if b"SIMPLEX_INTEROP_PERSISTENCE_PASS|same-contact-send-after-reboot" not in output:
                raise RuntimeError("guest did not report its persisted-contact send pass")
            if b"AGENT_RUNTIME|EXTERNAL_PERSISTENCE_BIDIRECTIONAL_PASS|same-contact-inbound-after-reboot" not in output:
                raise RuntimeError("guest did not report its post-reboot inbound pass")
        elif b"AGENT_RUNTIME|EXTERNAL_INTEROP_PASS|encrypted-cli-receive-ack" not in output:
            raise RuntimeError("guest exited cleanly without completing the one-way CLI interoperability check")
    finally:
        if qemu is not None and qemu.poll() is None:
            os.killpg(qemu.pid, signal.SIGTERM)
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(qemu.pid, signal.SIGKILL)
                qemu.wait()
        if master is not None:
            log = bytes(output).replace(b"\r", b"")
            (agent / "qemu-cli-interop.log").write_bytes(log)
            os.close(master)
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
        socks.shutdown()
        socks.server_close()

    if args.bidirectional:
        print("SIMPLEX AGENT CLI QEMU|PASS|initial bidirectional exchange and same-contact encrypted send after guest reboot")
    else:
        print("SIMPLEX AGENT CLI INTEROP QEMU|PASS|upstream CLI joined invitation, displayed encrypted guest message, acknowledged delivery")


if __name__ == "__main__":
    main()
