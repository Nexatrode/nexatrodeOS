"""QMP interaction checks for the disposable two-Slint-window fixture only."""
import json
import os
import re
import subprocess
import time


def run_headless(directory, interactive, expected=None, interaction=None):
    """Independent QEMU/HVF check. It opens no real VM, disk or network."""
    from utm_vault_key_storage import QmpPipe
    pipe = directory / "monitor"
    for suffix in (".in", ".out"):
        os.mkfifo(str(pipe) + suffix, 0o600)
    monitor = QmpPipe(pipe)
    trace = directory / "serial.log"
    command = ["qemu-system-aarch64", "-machine",
               "virt,virtualization=off,secure=off,gic-version=3,kernel-irqchip=on", "-accel", "hvf",
               "-cpu", "host", "-smp", "4", "-m", "2048", "-nodefaults", "-display", "none",
               "-monitor", "none", "-serial", f"file:{trace}", "-qmp", f"pipe:{pipe}",
               "-global", "virtio-mmio.force-legacy=false",
               "-device", "virtio-gpu-device,bus=virtio-mmio-bus.31,xres=1080,yres=2340,edid=off,blob=off,max_outputs=1,indirect_desc=off,event_idx=off",
               "-device", "virtio-keyboard-device,bus=virtio-mmio-bus.30,indirect_desc=off,event_idx=off",
               "-device", "virtio-tablet-device,bus=virtio-mmio-bus.29,indirect_desc=off,event_idx=off",
               "-device", f"loader,file={directory / 'loader.img'},addr=0x70000000,cpu-num=0"]
    with (directory / "qemu.log").open("w") as errors:
        process = subprocess.Popen(command, stdout=errors, stderr=errors)
        try:
            monitor.connect()
            deadline = time.monotonic() + 60
            expected = expected or ("RUNTIME|PASS|complete", "SURFACE|PASS|interactive ready" if interactive else "SURFACE|PASS|complete")
            while time.monotonic() < deadline:
                text = trace.read_text(errors="replace") if trace.exists() else ""
                if any(marker in text for marker in ("RUNTIME|FAIL|", "SURFACE|FAIL|", "MON|ERROR", "panicked at")) or process.poll() is not None:
                    raise RuntimeError("headless fixture failed: " + text[-2000:])
                if all(marker in text for marker in expected):
                    break
                time.sleep(.1)
            else:
                raise RuntimeError("headless fixture timed out: " + text[-2000:])
            if interaction:
                interaction(monitor, trace, directory, directory)
            elif interactive:
                exercise(monitor, trace, directory, directory)
            monitor.send("screendump", {"filename": str(directory / "screen.ppm")})
            print("PASS combined runtime and graphics in headless four-vCPU HVF")
        finally:
            monitor.close()
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


def exercise(monitor, trace, temporary, evidence):
    def output():
        text = trace.read_text(errors="replace")
        if any(marker in text for marker in ("INPUT|ERROR", "SURFACE|FAIL|", "RUNTIME|FAIL|",
                                               "MON|ERROR", "panicked at")):
            raise RuntimeError("interactive fixture failed: " + text[-3000:])
        # Debug output is emitted character by character. A higher-priority
        # display client can interrupt an app's line with this complete trace.
        # Strip only that known successful copy trace, after checking failures.
        return re.sub(r"GPU\|SLINT COPY rev=\d+ y=\d+ h=\d+\r?\n", "", text)

    def wait(marker):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if marker in output():
                return
            time.sleep(.05)
        raise RuntimeError("missing interactive marker " + marker + ": " + output()[-2000:])

    def keys(events):
        monitor.send("input-send-event", {"events": [
            {"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": key}}}
            for key, down in events]})

    def type_word(word):
        keys([(letter, down) for letter in word for down in (True, False)])

    def click(x, y):
        monitor.send("input-send-event", {"events": [
            {"type": "abs", "data": {"axis": "x", "value": x * 32767 // 1079}},
            {"type": "abs", "data": {"axis": "y", "value": y * 32767 // 2339}},
            {"type": "btn", "data": {"button": "left", "down": True}},
            {"type": "btn", "data": {"button": "left", "down": False}}]})

    def screen(name, focused, words):
        target = evidence / (name + ".ppm")
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            monitor.send("screendump", {"filename": str(temporary / "interaction.ppm")})
            contents = (temporary / "interaction.ppm").read_bytes()
            signature, dimensions, maximum, pixels = contents.split(b"\n", 3)
            if (signature, dimensions, maximum) != (b"P6", b"1080 2340", b"255"):
                raise RuntimeError("unexpected interactive framebuffer")
            if len(pixels) != 1080 * 2340 * 3:
                raise RuntimeError("incomplete interactive framebuffer")
            colours = []
            for x in (36, 556):
                offset = (496 * 1080 + x) * 3
                colours.append(tuple(pixels[offset:offset + 3]))
            expected = [(104, 221, 194) if focused == owner else (35, 53, 69) for owner in (1, 2)]
            if colours == expected:
                target.write_bytes(contents)
                text = subprocess.run(["tesseract", str(target), "stdout", "--psm", "11"],
                                      capture_output=True, text=True, check=True, timeout=30).stdout.lower()
                if all(word in text for word in words):
                    return
            output()
            time.sleep(.1)
        raise RuntimeError("framebuffer did not show expected focus and text: " + name)

    time.sleep(1)  # Final-state checks only, no host presentation latency claim.
    before = output().count("INPUT|KEY|")
    type_word("x")
    time.sleep(.2)
    if output().count("INPUT|KEY|") != before:
        raise RuntimeError("unfocused key reached an app")
    click(150, 700)
    type_word("alpha")
    wait("INPUT|TEXT|app=1|alpha")
    screen("focus-first", 1, ["alpha"])
    click(670, 700)
    type_word("bravo")
    wait("INPUT|TEXT|app=2|bravo")
    screen("focus-second", 2, ["alpha", "bravo"])

    # The release must go to the original owner after Tab changes focus.
    keys([("z", True)])
    wait("INPUT|KEY|app=2|code=44|value=1")
    keys([("tab", True), ("tab", False), ("z", False)])
    wait("INPUT|KEY|app=2|code=44|value=0")
    if "INPUT|KEY|app=1|code=44|" in output():
        raise RuntimeError("held key leaked into the newly focused app")
    type_word("q")
    wait("INPUT|TEXT|app=1|alphaq")
    screen("focus-returned", 1, ["alphaq", "bravoz"])
    click(100, 1500)
    time.sleep(.2)
    before = output().count("INPUT|KEY|")
    type_word("x")
    time.sleep(.2)
    if output().count("INPUT|KEY|") != before:
        raise RuntimeError("key reached app after clicking outside")
    screen("focus-cleared", 0, ["alphaq", "bravoz"])
    (evidence / "input-result.json").write_text(json.dumps({
        "checks": ["no-focus denial", "click focus", "separate typed text", "trusted focus border",
                   "Tab focus change", "held key release ownership", "outside click clears focus"],
        "observation": "QMP guest framebuffer and disposable app diagnostics",
        "latency_measurement": False,
    }, indent=2) + "\n")
    print("PASS isolated typing, focus changes, held-key ownership and guest framebuffer")
