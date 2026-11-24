#!/usr/bin/env python3
"""Exercise experimental lock UI and notes on a fresh UTM disk only."""
import argparse
import csv
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile
import time
import uuid

from utm_vault_key_storage import QmpPipe, checked, ctl


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--template", required=True, type=Path)
    parser.add_argument("--loader", required=True, type=Path)
    parser.add_argument("--work", required=True, type=Path)
    parser.add_argument("--check-nonblank", action="store_true", help="verify enrollment refuses a disposable disk containing a sentinel")
    parser.add_argument("--check-calculator", action="store_true", help="test Calculator touch input on a fresh disposable disk")
    parser.add_argument("--require-agent", action="store_true", help="verify SimpleX startup waits for the vault and becomes ready")
    args = parser.parse_args()
    work = args.work.resolve()
    if any(p.suffix == ".utm" for p in (work, *work.parents)):
        raise ValueError("test work directory cannot be inside a VM bundle")
    work.mkdir(parents=True, exist_ok=True)
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix="wrapped-lock-", dir=work, suffix=".utm") as tmp:
        bundle = Path(tmp); data = bundle / "Data"; data.mkdir()
        tag, drive = str(uuid.uuid4()).upper(), str(uuid.uuid4()).upper()
        config = plistlib.loads(args.template.read_bytes())
        assert len(config["Drive"]) == 1 and config["Network"] == []
        assert config["System"]["CPUCount"] == 4 and config["QEMU"]["Hypervisor"] is True
        config["Information"].update(UUID=tag, Name="Disposable wrapped lock test")
        config["Serial"] = []
        config["Sharing"].update(ClipboardSharing=False, DirectoryShareMode="None")
        config["QEMU"]["DebugLog"] = False
        old_drive = config["Drive"][0]["Identifier"]
        config["Drive"][0].update(Identifier=drive, ImageName="blank.img")
        opts = config["QEMU"]["AdditionalArguments"]
        assert sum(o.startswith("-device loader,") for o in opts) == 1
        assert sum(o.startswith("-device virtio-blk-device,") for o in opts) == 1
        assert sum(o.startswith("-netdev user,id=net0") for o in opts) == 1
        allowed = ("-global virtio-", "-device loader,", "-device virtio-")
        assert all(o.startswith(allowed) or o.startswith("-netdev user,id=net0") for o in opts)
        for i, option in enumerate(opts):
            if option.startswith("-device loader,"):
                opts[i] = f"-device loader,file={data}/loader.img,addr=0x70000000,cpu-num=0"
            elif option.startswith("-device virtio-blk-device,"):
                opts[i] = option.replace(old_drive, drive)
            elif option.startswith("-netdev user,id=net0"):
                opts[i] = "-netdev user,id=net0,restrict=on"
        trace, pipe = data / "trace.log", data / "monitor"
        for ext in (".in", ".out"): os.mkfifo(str(pipe) + ext, 0o600)
        opts.extend([f"-serial file:{trace}", f"-qmp pipe:{pipe}"])
        (bundle / "config.plist").write_bytes(plistlib.dumps(config))
        shutil.copyfile(args.loader, data / "loader.img")
        disk = data / "blank.img"
        with disk.open("wb") as disk_file: disk_file.truncate(16 * 1024 * 1024)
        checked("sgdisk", "--clear", "--new=1:2048:0", "--typecode=1:0700", disk)
        if args.check_nonblank:
            with disk.open("r+b") as disk_file:
                disk_file.seek(2048 * 512 + 100 * 4096)
                disk_file.write(b"disposable-nonblank-sentinel")
                disk_file.flush(); os.fsync(disk_file.fileno())
        registered = started = False
        monitor = None

        def start():
            nonlocal started, monitor
            trace.unlink(missing_ok=True)
            monitor = QmpPipe(pipe)
            result = ctl("start", "--hide", tag)
            if result.returncode: raise RuntimeError("could not start fixture: " + result.stderr)
            started = True
            monitor.connect()

        def stop():
            nonlocal started, monitor
            if monitor: monitor.close()
            monitor = None
            if ctl("stop", tag, "--force").returncode: raise RuntimeError("could not stop fixture")
            for _ in range(100):
                if ctl("status", tag).stdout.strip() == "stopped":
                    started = False
                    return
                time.sleep(.2)
            raise RuntimeError("fixture did not stop")

        def screen():
            image = data / "screen.ppm"
            image.unlink(missing_ok=True)
            monitor.send("screendump", {"filename": str(image)})
            if not image.exists() or image.stat().st_size < 7_581_600: return []
            text = subprocess.run(["tesseract", str(image), "stdout", "--psm", "6", "tsv"],
                                  capture_output=True, text=True, check=True, timeout=60).stdout
            return [(r["text"].strip().lower(), int(r["left"]), int(r["top"]),
                     int(r["width"]), int(r["height"])) for r in csv.DictReader(text.splitlines(), delimiter="\t")
                    if r["text"].strip()]

        def marker_index(output, marker):
            # Multiple guest CPUs can interleave their debug output mid-line.
            if marker == "SIMPLEX_GATE|WAITING":
                match = re.search(r"SIMPLEX_GATE[\s\S]{0,4096}?\|WAITING", output)
                # During early boot even the prefix can be interrupted; the
                # gate is the sole producer of this suffix in the fixture.
                return match.start() if match else output.find("|WAITING\n")
            return output.find(marker)

        def wait_log(marker, timeout=90, occurrences=1, since=0):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                output = trace.read_text(errors="replace") if trace.exists() else ""
                faults = re.findall(r"MON\|ERROR: faulting PD: ([\w_]+)", output)
                if any(args.require_agent or pd != "simplex_agent" for pd in faults) or re.search(r"(?:LDR\|ERROR|panicked at|Kernel panic|Assertion failed)", output) or (args.require_agent and "HASKELL_EXIT|" in output):
                    raise RuntimeError("guest fault: " + output[-3000:])
                recent = output[since:]
                if (marker_index(recent, marker) >= 0 and
                        (occurrences == 1 or recent.count(marker) >= occurrences)): return
                time.sleep(.2)
            raise RuntimeError("log marker missing: " + marker)

        def wait(word, timeout=120):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                rows = screen()
                if word.lower() == "saved" and any("failed" in item[0] for item in rows):
                    raise RuntimeError("note save reported failure")
                if word.lower() != "unavailable" and any("unavailable" in item[0] for item in rows) and any("unlock" in item[0] for item in rows):
                    raise RuntimeError("lock screen reports storage unavailable")
                for item in rows:
                    if word.lower() in item[0]: return item
                output = trace.read_text(errors="replace") if trace.exists() else ""
                faults = re.findall(r"MON\|ERROR: faulting PD: ([\w_]+)", output)
                if any(args.require_agent or pd != "simplex_agent" for pd in faults) or re.search(r"(?:LDR\|ERROR|panicked at|Kernel panic|Assertion failed)", output) or (args.require_agent and "HASKELL_EXIT|" in output):
                    raise RuntimeError("guest fault: " + output[-3000:])
                time.sleep(.3)
            raise RuntimeError("screen text not found: " + word)

        def click_at(x, y):
            events = [{"type":"abs", "data":{"axis":axis,"value":int(value*32767/size)}}
                      for axis,value,size in (("x",x,1080),("y",y,2340))]
            events.extend({"type":"btn", "data":{"button":"left","down":down}} for down in (True,False))
            monitor.send("input-send-event", {"events": events})
            time.sleep(.3)

        def wait_note(timeout=120):
            # OCR commonly confuses I/F/C in the fixture. Its unique prefix
            # must appear in the note body, above the status and keyboard.
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if any(item[0].startswith("vault") and item[2] < 1000 for item in screen()):
                    return
                time.sleep(.3)
            raise RuntimeError("saved fixture note was not visible")

        def click(word):
            _,x,y,w,h = wait(word)
            click_at(x+w/2, y+h/2)

        def key(name):
            events = [{"type":"key", "data":{"down":down,"key":{"type":"qcode","data":name}}}
                      for down in (True,False)]
            monitor.send("input-send-event", {"events":events})
            time.sleep(.06)

        def type_text(value):
            for char in value: key(char)

        def swipe_up():
            events = [{"type":"abs","data":{"axis":"x","value":int(540*32767/1080)}},
                      {"type":"abs","data":{"axis":"y","value":int(2050*32767/2340)}},
                      {"type":"btn","data":{"button":"left","down":True}}]
            monitor.send("input-send-event", {"events":events})
            for y in (1950,1850,1750,1650,1550):
                monitor.send("input-send-event", {"events":[{"type":"abs","data":{"axis":"y","value":int(y*32767/2340)}}]})
                time.sleep(.08)
            monitor.send("input-send-event", {"events":[{"type":"btn","data":{"button":"left","down":False}}]})
            wait("passphrase")

        def passphrase(value):
            type_text(value); key("ret")

        def privacy():
            if not any("simplex" in row[0] for row in screen()): click("home"); wait("simplex")
            click("settings"); click("privacy"); wait("relays")

        def notes():
            if not any("tools" in row[0] for row in screen()): click("home"); wait("simplex")
            click("tools"); click("notes"); wait("notes")

        def check_waiting():
            if not args.require_agent: return
            wait_log("SIMPLEX_GATE|WAITING", 180)
            output = trace.read_text(errors="replace")
            if any(marker in output for marker in ("SIMPLEX_GATE|STARTING", "AGENT_RUNTIME|SERVICE_READY", "SIMPLEX_VAULT|REQ|")):
                raise RuntimeError("SimpleX accessed the vault before unlock")

        def check_ready():
            if not args.require_agent: return
            wait_log("AGENT_RUNTIME|SERVICE_READY", 180)
            output = trace.read_text(errors="replace")
            for marker in ("SIMPLEX_GATE|WAITING", "WRAPPED_VAULT|UNLOCKED", "SIMPLEX_GATE|STARTING", "AGENT_RUNTIME|SERVICE_READY"):
                if marker_index(output, marker) < 0: raise RuntimeError("missing startup marker: " + marker)
            if not (marker_index(output, "SIMPLEX_GATE|WAITING") < output.index("WRAPPED_VAULT|UNLOCKED")
                    < output.index("SIMPLEX_GATE|STARTING") < output.index("AGENT_RUNTIME|SERVICE_READY")):
                raise RuntimeError("SimpleX started in the wrong order")
            print("PASS: SimpleX started after vault unlock", flush=True)

        try:
            checked("open", "-g", "-a", "UTM", bundle)
            for _ in range(120):
                if any(tag in line and "stopped" in line for line in ctl("list").stdout.splitlines()):
                    registered = True; break
                time.sleep(.25)
            if not registered: raise RuntimeError("fixture not registered")
            start(); check_waiting(); wait("home"); click("home"); wait("simplex")
            if args.check_calculator:
                click("tools"); click("calculator"); wait("calculator")
                # Centers of 7, +, 5, = in the 1080x2340 fixture.
                for x, y in ((163, 815), (905, 1465), (410, 1035), (658, 1465)):
                    click_at(x, y)
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    if any(item[0] == "12" and 460 <= item[2] <= 690 for item in screen()):
                        print("PASS: Calculator touch input displayed 7 + 5 = 12", flush=True)
                        return
                    time.sleep(.3)
                raise RuntimeError("Calculator did not display 12")
            privacy(); click("passphrase"); wait("passphrase")
            passphrase("fixture42"); wait("confirm"); passphrase("fixture42")
            if args.check_nonblank:
                wait("unavailable", 180); wait_log("WRAPPED_VAULT|NONBLANK|")
                stop()
                with disk.open("rb") as disk_file:
                    disk_file.seek(2048 * 512)
                    if any(disk_file.read(8192)): raise RuntimeError("bootstrap was written to nonblank disk")
                    disk_file.seek(2048 * 512 + 100 * 4096)
                    if disk_file.read(len(b"disposable-nonblank-sentinel")) != b"disposable-nonblank-sentinel":
                        raise RuntimeError("nonblank sentinel changed")
                print("PASS: nonblank disposable disk was refused and preserved", flush=True)
                return
            wait("locked", 180)
            print("PASS: enrollment reached locked screen", flush=True)
            swipe_up(); passphrase("wrongtest"); wait("unable"); check_waiting(); time.sleep(2)
            passphrase("fixture42"); wait("settings", 180); wait_log("WRAPPED_VAULT|UNLOCKED")
            check_ready()
            print("PASS: wrong passphrase rejected, correct passphrase unlocked vault", flush=True)
            if args.require_agent:
                click("simplex"); wait("join", 180)
                wait_log("SIMPLEXIPC|MESSAGES_APP|LIST_CONNECTIONS_PASS|0", 180)
                print("PASS: SimpleX read the empty disposable chat list", flush=True)
            notes(); click_at(925,1445); wait("save")
            for _ in range(48): key("backspace")
            type_text("vaultfixture"); wait_note(); click("save"); wait("saved")
            print("PASS: disposable note saved through vault", flush=True)
            click("list"); privacy(); click("lock"); wait("locked"); wait_log("WRAPPED_VAULT|LOCKED")
            print("PASS: manual lock removed the vault key", flush=True)
            if args.require_agent:
                # Exercise a second unlock without restarting the agent or VM.
                time.sleep(3)
                swipe_up(); passphrase("fixture42"); wait("settings", 180)
                wait_log("WRAPPED_VAULT|UNLOCKED", 180, occurrences=2)
                after_unlock = len(trace.read_text(errors="replace"))
                click("simplex"); wait("join", 180)
                wait_log("VAULT|FILE_READ|channel=Channel { index: 7 }", 180, since=after_unlock)
                wait_log("SIMPLEXIPC|MESSAGES_APP|RECEIVE_EMPTY", 180, since=after_unlock)
                print("PASS: SimpleX resumes database reads and polling after re-lock and unlock", flush=True)
            stop()
            if b"VAULTFIXTURE" in disk.read_bytes(): raise RuntimeError("test note was found in plaintext on disposable disk")
            start(); check_waiting(); wait("locked", 180); swipe_up(); passphrase("wrongtest"); wait("unable"); check_waiting()
            if "WRAPPED_VAULT|UNLOCKED" in trace.read_text(errors="replace"):
                raise RuntimeError("vault unlocked after wrong passphrase")
            time.sleep(2)
            passphrase("fixture42"); wait("settings", 180); wait_log("WRAPPED_VAULT|UNLOCKED")
            check_ready()
            notes(); wait_note(180)
            print("PASS: reboot required passphrase and recovered disposable note", flush=True)
            stop()
            with disk.open("r+b") as disk_file:
                disk_file.seek(2048 * 512)
                slots = disk_file.read(8192)
                candidates = [(int.from_bytes(slots[i*4096+12:i*4096+20], "little"), i)
                              for i in (0, 1) if slots[i*4096:i*4096+8] == b"PVLTBLB1"]
                if len(candidates) != 2: raise RuntimeError("expected two bootstrap slots")
                generation, slot = max(candidates)
                length = int.from_bytes(slots[slot*4096+20:slot*4096+24], "little")
                marker = 2048 * 512 + slot * 4096 + 36 + length + 16
                disk_file.seek(marker)
                disk_file.write((generation ^ 0x3c6ef372fe94f82b ^ 1).to_bytes(8, "little"))
                disk_file.flush(); os.fsync(disk_file.fileno())
            start(); check_waiting(); wait("locked", 180); swipe_up(); passphrase("fixture42"); wait("settings", 180)
            wait_log("WRAPPED_VAULT|UNLOCKED"); check_ready(); notes(); wait_note(180)
            print("PASS: older bootstrap slot recovered after interrupted replacement", flush=True)
            print("Waiting for the 60-second auto-lock…", flush=True)
            wait("locked", 100); wait_log("WRAPPED_VAULT|LOCKED")
            print("PASS: auto-lock removed the vault key", flush=True)
        finally:
            if trace.exists(): shutil.copyfile(trace, work / "last-trace.log")
            if (data / "screen.ppm").exists(): shutil.copyfile(data / "screen.ppm", work / "last-screen.ppm")
            if started: stop()
            if registered: ctl("delete", tag)


if __name__ == "__main__":
    main()
