#!/usr/bin/env python3
"""Exercise wrapped-key blob commits in isolated accelerated UTM guests.

Inputs are test-only Microkit images/configuration. Every case creates its own
blank GPT disk. No real VM disk or provisioned vault image is read or copied.
"""
import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import struct
import subprocess
import tempfile
import time
import uuid

CTL = "/Applications/UTM.app/Contents/MacOS/utmctl"
OBJCOPY = "/opt/homebrew/opt/llvm/bin/llvm-objcopy"
BOUNDARIES = {1: "BEFORE_STAGE_FLUSH", 2: "AFTER_STAGE_FLUSH",
              3: "BEFORE_COMMIT_FLUSH", 4: "AFTER_COMMIT_FLUSH"}
ARTIFACTS = ("storage.system", "rust_vault_disk.elf", "rust_vault_test_client.elf",
             "rust_entropy_rng.elf", "blk_driver.elf", "blk_virt.elf",
             "serial_driver.elf", "serial_virt_tx.elf")


def ctl(*args):
    return subprocess.run([CTL, *args], capture_output=True, text=True, timeout=30)


def checked(*args):
    subprocess.run([str(arg) for arg in args], check=True, capture_output=True)


def build_loader(point, artifacts, sdk, directory):
    for name in ARTIFACTS:
        shutil.copyfile(artifacts / name, directory / name)
    config = directory / "point.bin"
    config.write_bytes(struct.pack("<I", point))
    checked(OBJCOPY, "--update-section", f".storage_test_config={config}",
            directory / "rust_vault_test_client.elf")
    checked(sdk / "bin/microkit", directory / "storage.system", "--search-path", directory,
            "--board", "qemu_virt_aarch64_el1", "--config", "smp-debug",
            "-o", directory / "loader.img", "-r", directory / "report.txt")


class QmpPipe:
    def __init__(self, path):
        self.output = os.open(str(path) + ".out", os.O_RDONLY | os.O_NONBLOCK)
        self.input = None
        self.buffer = b""
        self.next_id = 0
        self.path = path

    def connect(self):
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            try:
                self.input = os.open(str(self.path) + ".in", os.O_WRONLY | os.O_NONBLOCK)
                self.send("qmp_capabilities")
                return
            except OSError:
                time.sleep(.2)
        raise RuntimeError("QMP did not start")

    def send(self, command, arguments=None):
        self.next_id += 1
        payload = {"execute": command, "id": self.next_id}
        if arguments is not None: payload["arguments"] = arguments
        os.write(self.input, (json.dumps(payload) + "\n").encode())
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            try:
                self.buffer += os.read(self.output, 65536)
            except BlockingIOError:
                pass
            while b"\n" in self.buffer:
                line, self.buffer = self.buffer.split(b"\n", 1)
                try:
                    reply = json.loads(line)
                except (ValueError, UnicodeDecodeError):
                    continue
                if reply.get("id") == self.next_id:
                    if "error" in reply:
                        raise RuntimeError(f"QMP {command} rejected: {reply['error']}")
                    return reply
            time.sleep(.02)
        raise RuntimeError("QMP command timed out: " + command)

    def close(self):
        if self.input is not None: os.close(self.input)
        os.close(self.output)


def wait_marker(trace, marker, after=0):
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        output = trace.read_text(errors="replace") if trace.exists() else ""
        current = output[after:]
        if any(x in current for x in ("KEY_STORAGE|FAIL|", "MON|ERROR", "Kernel panic")):
            raise RuntimeError("guest fault: " + current[-4000:])
        if marker in current:
            return after + current.index(marker) + len(marker)
        time.sleep(.1)
    raise RuntimeError("missing " + marker + ": " + output[-4000:])


def run_case(point, template, artifacts, sdk):
    with tempfile.TemporaryDirectory(prefix="key-store-") as root:
        root = Path(root)
        case = root / "case"
        case.mkdir()
        build_loader(point, artifacts, sdk, case)
        bundle = root / "guest.utm"
        data = bundle / "Data"
        data.mkdir(parents=True)
        shutil.copyfile(case / "loader.img", data / "loader.img")
        disk = data / "blank.img"
        with disk.open("wb") as handle:
            handle.truncate(16 * 1024 * 1024)
        checked("sgdisk", "--clear", "--new=1:2048:0", "--typecode=1:0700", disk)
        tag, drive = str(uuid.uuid4()).upper(), str(uuid.uuid4()).upper()
        config = plistlib.loads(template.read_bytes())
        assert len(config["Drive"]) == 1 and config["Network"] == []
        assert config["System"]["CPUCount"] == 4 and config["QEMU"]["Hypervisor"] is True
        config["Information"].update(UUID=tag, Name=f"Disposable vault storage test {point}")
        config["Serial"] = []
        config["Sharing"].update(ClipboardSharing=False, DirectoryShareMode="None")
        config["QEMU"]["DebugLog"] = False
        original_drive = config["Drive"][0]["Identifier"]
        config["Drive"][0].update(Identifier=drive, ImageName="blank.img")
        options = config["QEMU"]["AdditionalArguments"]
        assert sum(o.startswith("-device loader,") for o in options) == 1
        assert sum(o.startswith("-device virtio-blk-device,") for o in options) == 1
        assert sum(o.startswith("-netdev user,id=net0") for o in options) == 1
        allowed = ("-global virtio-", "-device loader,", "-device virtio-")
        assert all(o.startswith(allowed) or o.startswith("-netdev user,id=net0") for o in options)
        for i, option in enumerate(options):
            if option.startswith("-device loader,"):
                options[i] = f"-device loader,file={data}/loader.img,addr=0x70000000,cpu-num=0"
            elif option.startswith("-device virtio-blk-device,"):
                options[i] = option.replace(original_drive, drive)
            elif option.startswith("-netdev user,id=net0"):
                options[i] = "-netdev user,id=net0,restrict=on"
        trace, qmp_path = data / "trace.log", data / "monitor"
        for suffix in (".in", ".out"):
            os.mkfifo(str(qmp_path) + suffix, 0o600)
        options.extend([f"-serial file:{trace}", f"-qmp pipe:{qmp_path}"])
        (bundle / "config.plist").write_bytes(plistlib.dumps(config))
        registered = started = False
        monitor = None

        def start():
            nonlocal started, monitor
            trace.unlink(missing_ok=True)
            monitor = QmpPipe(qmp_path)
            result = ctl("start", "--hide", tag)
            if result.returncode:
                raise RuntimeError("could not start disposable VM: " + result.stderr)
            started = True
            monitor.connect()

        def stop():
            nonlocal started, monitor
            if monitor: monitor.close()
            monitor = None
            if ctl("stop", tag, "--force").returncode:
                raise RuntimeError("could not stop disposable VM")
            for _ in range(100):
                if ctl("status", tag).stdout.strip() == "stopped":
                    started = False
                    return
                time.sleep(.2)
            raise RuntimeError("disposable VM did not stop")

        try:
            checked("open", "-g", "-a", "UTM", bundle)
            for _ in range(120):
                if any(tag in line and "stopped" in line for line in ctl("list").stdout.splitlines()):
                    registered = True
                    break
                time.sleep(.25)
            if not registered: raise RuntimeError("fixture not registered")
            start()
            offset = wait_marker(trace, "KEY_STORAGE|BASELINE_OK")
            if point:
                offset = wait_marker(trace, "STORAGE|RESET_POINT|" + BOUNDARIES[point], after=offset)
                monitor.send("system_reset")
                recovery = f"KEY_STORAGE|RECOVERY_OK|point={point}|generation="
                wait_marker(trace, recovery, after=offset)
                deadline = time.monotonic() + 5
                match = None
                while not match and time.monotonic() < deadline:
                    match = re.search(re.escape(recovery) + r"([12])", trace.read_text(errors="replace"))
                    if not match: time.sleep(.05)
                if not match: raise RuntimeError("recovery generation missing")
                recovered_generation = match.group(1)
            else:
                monitor.send("system_reset")
                offset = wait_marker(trace, "KEY_STORAGE|REPLACED_OK", after=offset)
                monitor.send("system_reset")
                wait_marker(trace, "KEY_STORAGE|NORMAL_REOPEN_OK", after=offset)
            stop()
            start()
            wait_marker(trace, (recovery + recovered_generation) if point else "KEY_STORAGE|NORMAL_REOPEN_OK")
            stop()
            detail = ("normal replacement" if point == 0 else
                      BOUNDARIES[point] + ", generation " + recovered_generation)
            print(f"PASS: {detail} and cold reopen", flush=True)
        finally:
            if started: stop()
            if registered: ctl("delete", tag)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--template", required=True, type=Path)
    parser.add_argument("--artifacts", required=True, type=Path)
    parser.add_argument("--sdk", required=True, type=Path)
    parser.add_argument("--case", type=int, choices=range(5), help="run one case instead of all five")
    args = parser.parse_args()
    os.umask(0o077)
    for point in ([args.case] if args.case is not None else range(5)):
        run_case(point, args.template, args.artifacts, args.sdk)


if __name__ == "__main__":
    main()
