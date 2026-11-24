#!/usr/bin/env python3
"""Boot the isolated key prototype in a disposable accelerated UTM VM.

The template is read for VM settings only. No existing drive image is opened;
the temporary VM uses a fresh blank image and restricted networking.
"""
import argparse
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import time
import uuid


CTL = "/Applications/UTM.app/Contents/MacOS/utmctl"


def ctl(*args):
    return subprocess.run([CTL, *args], capture_output=True, text=True, timeout=30)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--template", required=True, type=Path, help="existing VM config.plist, read-only")
    parser.add_argument("--loader", required=True, type=Path, help="standalone probe loader")
    parser.add_argument("--expect", action="append", help="guest marker required for a generic disposable probe")
    parser.add_argument("--reject", action="append", help="guest marker that fails a generic disposable probe")
    parser.add_argument("--blank-gpt", action="store_true",
                        help="partition the newly created scratch disk for a vault integration test")
    parser.add_argument("--serial-output", type=Path, help="save this disposable guest's serial output")
    parser.add_argument("--screenshot-output", type=Path, help="capture the disposable guest framebuffer as PPM")
    parser.add_argument("--desktop-input-test", action="store_true",
                        help="exercise the disposable two-window input fixture through QMP")
    parser.add_argument("--desktop-lock-test", choices=("normal", "skip-scrub", "wrapped", "skip-evict"),
                        help="exercise passphrase-gated dynamic processes using a fresh disk")
    parser.add_argument("--desktop-notes-test", action="store_true", help="test dynamic private Notes and interrupted saves on a fresh disk")
    parser.add_argument("--desktop-simplex-test", action="store_true", help="test parent-managed SimpleX lock recovery offline on a fresh disk")
    parser.add_argument("--desktop-lifecycle-test", choices=("normal", "skip-revoke", "keep-endpoint"),
                        help="exercise a dynamically created graphical process")
    args = parser.parse_args()
    if (args.desktop_input_test or args.desktop_lifecycle_test or args.desktop_lock_test or args.desktop_notes_test or args.desktop_simplex_test) and not args.screenshot_output:
        parser.error("the desktop input test requires an evidence screenshot destination")
    if args.desktop_input_test and args.desktop_lifecycle_test:
        parser.error("choose one disposable interaction fixture")
    if args.desktop_lock_test and (not args.blank_gpt or args.desktop_input_test or args.desktop_lifecycle_test):
        parser.error("the lock fixture requires its own fresh GPT disk and exclusive callback")
    if args.desktop_notes_test and (not args.blank_gpt or args.desktop_input_test or args.desktop_lifecycle_test or args.desktop_lock_test):
        parser.error("the Notes fixture requires a fresh GPT disk and exclusive callback")
    if args.desktop_simplex_test and (not args.blank_gpt or args.desktop_input_test or args.desktop_lifecycle_test or args.desktop_lock_test or args.desktop_notes_test):
        parser.error("the SimpleX fixture requires its own fresh GPT disk and exclusive callback")
    state = subprocess.run(["ioreg", "-n", "Root", "-d", "1", "-a"],
                           capture_output=True, check=True, timeout=10)
    console = plistlib.loads(state.stdout)
    entries = console if isinstance(console, list) else [console]
    if any(user.get("CGSSessionScreenIsLocked", False)
           for entry in entries for user in entry.get("IOConsoleUsers", [])):
        raise RuntimeError("unlock the Mac before running UTM's control interface")
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix="vault-key-probe-", suffix=".utm") as tmp:
        bundle = Path(tmp)
        data = bundle / "Data"
        data.mkdir()
        tag = str(uuid.uuid4()).upper()
        drive = str(uuid.uuid4()).upper()
        config = plistlib.loads(args.template.read_bytes())
        assert len(config["Drive"]) == 1 and config["Network"] == []
        assert config["System"]["CPUCount"] == 4 and config["QEMU"]["Hypervisor"] is True
        config["Information"].update(UUID=tag, Name="Disposable vault key probe")
        config["Serial"] = []
        config["Sharing"].update(ClipboardSharing=False, DirectoryShareMode="None")
        config["QEMU"]["DebugLog"] = False
        old_drive = config["Drive"][0]["Identifier"]
        config["Drive"][0].update(Identifier=drive, ImageName="blank.img")
        options = config["QEMU"]["AdditionalArguments"]
        assert sum(o.startswith("-device loader,") for o in options) == 1
        assert sum(o.startswith("-device virtio-blk-device,") for o in options) == 1
        assert sum(o.startswith("-netdev user,id=net0") for o in options) == 1
        # Reject unexpected template arguments instead of risking an extra host
        # file or device being passed through to this disposable guest.
        allowed = ("-global virtio-", "-device loader,", "-device virtio-")
        assert all(o.startswith(allowed) or o.startswith("-netdev user,id=net0") for o in options)
        for index, option in enumerate(options):
            if option.startswith("-device loader,"):
                options[index] = f"-device loader,file={data}/loader.img,addr=0x70000000,cpu-num=0"
            elif option.startswith("-device virtio-blk-device,"):
                options[index] = option.replace(old_drive, drive)
            elif option.startswith("-netdev user,id=net0"):
                options[index] = "-netdev user,id=net0,restrict=on"
        trace = data / "trace.log"
        options.append("-serial file:" + str(trace))
        monitor = None
        if args.screenshot_output:
            from utm_vault_key_storage import QmpPipe
            monitor_path = data / "monitor"
            for suffix in (".in", ".out"):
                os.mkfifo(str(monitor_path) + suffix, 0o600)
            options.append("-qmp pipe:" + str(monitor_path))
            monitor = QmpPipe(monitor_path)
        (bundle / "config.plist").write_bytes(plistlib.dumps(config))
        shutil.copyfile(args.loader, data / "loader.img")
        with (data / "blank.img").open("wb") as disk:
            disk.truncate(16 * 1024 * 1024)
        if args.blank_gpt:
            subprocess.run(["sgdisk", "--clear", "--new=1:2048:0", "--typecode=1:0700",
                            data / "blank.img"], check=True, stdout=subprocess.DEVNULL)

        registered = False
        started = False
        try:
            print("Disposable UTM UUID " + tag, flush=True)
            subprocess.run(["open", "-g", "-a", "UTM", bundle], check=True)
            for _ in range(120):
                if any(tag in line and "stopped" in line for line in ctl("list").stdout.splitlines()):
                    registered = True
                    break
                time.sleep(0.25)
            if not registered:
                raise RuntimeError("disposable VM did not register")
            result = ctl("start", "--hide", tag)
            if result.returncode:
                raise RuntimeError("disposable VM did not start: " + result.stderr)
            started = True
            if monitor:
                monitor.connect()
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline:
                output = trace.read_text(errors="replace") if trace.exists() else ""
                expected = args.expect or ["KEY_PROBE|PASS|roundtrip_wrong_password_tamper"]
                rejected = args.reject if args.reject is not None else ["KEY_PROBE|FAIL|", "MON|ERROR"]
                if any(marker in output for marker in rejected):
                    raise RuntimeError("guest reported failure: " + output[-3000:])
                if all(marker in output for marker in expected):
                    if args.desktop_simplex_test:
                        from desktop_simplex_input import exercise
                        exercise(monitor, trace, data, args.screenshot_output.parent)
                    if args.desktop_notes_test:
                        from desktop_notes_input import exercise
                        exercise(monitor, trace, data, args.screenshot_output.parent)
                    if args.desktop_lock_test:
                        from desktop_lock_input import exercise
                        exercise(monitor, trace, data, args.screenshot_output.parent,
                                 control=args.desktop_lock_test == "skip-scrub",
                                 wrapped=args.desktop_lock_test in ("wrapped", "skip-evict"),
                                 eviction_control=args.desktop_lock_test == "skip-evict")
                    if args.desktop_input_test:
                        from desktop_input_utm import exercise
                        exercise(monitor, trace, data, args.screenshot_output.parent)
                    if args.desktop_lifecycle_test:
                        from desktop_graphical_input import exercise
                        exercise(monitor, trace, data, args.screenshot_output.parent,
                                 control=None if args.desktop_lifecycle_test == "normal" else args.desktop_lifecycle_test)
                    if monitor:
                        # The guest marker precedes asynchronous device presentation.
                        # This capture is a final-state check and carries no latency claim.
                        time.sleep(1)
                        capture = data / "frame.ppm"
                        monitor.send("screendump", {"filename": str(capture)})
                        shutil.copyfile(capture, args.screenshot_output)
                    print("UTM probe passed: " + ", ".join(expected))
                    return
                time.sleep(0.25)
            raise RuntimeError("probe timed out: " + output[-3000:])
        finally:
            if monitor:
                monitor.close()
            if started:
                ctl("stop", tag, "--force")
                for _ in range(100):
                    if ctl("status", tag).stdout.strip() == "stopped":
                        break
                    time.sleep(0.2)
                else:
                    raise RuntimeError("disposable VM did not stop")
            try:
                if args.serial_output and trace.exists():
                    args.serial_output.write_bytes(trace.read_bytes())
            finally:
                if registered and ctl("delete", tag).returncode:
                    raise RuntimeError("could not unregister disposable VM")


if __name__ == "__main__":
    main()
