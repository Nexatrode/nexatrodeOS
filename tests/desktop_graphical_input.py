"""Disposable dynamic-window interaction and lifecycle checks through QMP."""
import json
import re
import subprocess
import time


def exercise(monitor, trace, temporary, evidence, control=None):
    expected_failure = {"skip-revoke": "RUNTIME|FAIL|object capability survived error=0",
                        "keep-endpoint": "RUNTIME|FAIL|escaped capability survived error=0"}.get(control)
    def output():
        text = trace.read_text(errors="replace")
        text = re.sub(r"GPU\|SLINT COPY rev=\d+ y=\d+ h=\d+\r?\n", "", text)
        text = re.sub(r"GPU_VIRTIO_DRIVER\|ERROR: Received interrupt but is neither a used buffer notification nor a gpu event, irq status: 0x0\r?\n", "", text)
        failures = re.findall(r"RUNTIME\|FAIL\|[^\r\n]*", text)
        if any(failure != expected_failure for failure in failures):
            raise RuntimeError("dynamic runtime failed: " + text[-3000:])
        if any(marker in text for marker in ("SURFACE|FAIL|", "INPUT|ERROR", "MON|ERROR", "panicked at", "Kernel panic", "DYNAMIC|UNEXPECTED_FAULT|")):
            raise RuntimeError("dynamic graphical fixture failed: " + text[-3000:])
        return text

    def wait(marker):
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            if marker in output().splitlines(): return
            time.sleep(.05)
        raise RuntimeError("missing dynamic marker " + marker + ": " + output()[-2000:])

    def keys(events):
        monitor.send("input-send-event", {"events": [
            {"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": key}}}
            for key, down in events]})

    def type_word(word): keys([(character, down) for character in word for down in (True, False)])
    def function(number): keys([("f" + str(number), True), ("f" + str(number), False)])
    def click(x, y):
        monitor.send("input-send-event", {"events": [
            {"type": "abs", "data": {"axis": "x", "value": x * 32767 // 1079}},
            {"type": "abs", "data": {"axis": "y", "value": y * 32767 // 2339}},
            {"type": "btn", "data": {"button": "left", "down": True}},
            {"type": "btn", "data": {"button": "left", "down": False}}]})

    def screen(name, words, closed=False, absent=()):
        time.sleep(1)  # Observe a settled guest frame; this test makes no latency claim.
        path = evidence / (name + ".ppm")
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            monitor.send("screendump", {"filename": str(temporary / "dynamic.ppm")})
            contents = (temporary / "dynamic.ppm").read_bytes()
            signature, dimensions, maximum, pixels = contents.split(b"\n", 3)
            if (signature, dimensions, maximum) != (b"P6", b"1080 2340", b"255") or len(pixels) != 1080 * 2340 * 3:
                raise RuntimeError("invalid dynamic framebuffer")
            if closed:
                background = bytes((4, 12, 22)) * 480
                if any(pixels[(row * 1080 + 40) * 3:(row * 1080 + 520) * 3] != background for row in range(500, 1300)):
                    time.sleep(.1)
                    continue
            path.write_bytes(contents)
            text = subprocess.run(["tesseract", str(path), "stdout", "--psm", "11"],
                                  capture_output=True, text=True, check=True, timeout=30).stdout.lower()
            if all(word in text for word in words) and all(word not in text for word in absent): return
            output()
            time.sleep(.1)
        raise RuntimeError("dynamic framebuffer mismatch: " + name)

    wait("DYNAMIC|READY|generation=1")
    click(150, 700); type_word("alpha")
    wait("INPUT|TEXT|app=1|alpha")
    click(670, 700); type_word("bravo")
    wait("INPUT|TEXT|app=2|bravo")
    screen("dynamic-first", ("alpha", "bravo"))
    click(400, 700); keys([("z", True)])
    wait("INPUT|TEXT|app=1|alphaz")
    function(5)
    if control:
        wait(expected_failure)
        (evidence / "lifecycle-result.json").write_text(json.dumps({
            "negative_control": control, "detected_failure": expected_failure,
            "production_security_claim": False}, indent=2) + "\n")
        print("PASS negative control detected " + control)
        return

    wait("DYNAMIC|CLOSED|generation=1")
    screen("dynamic-closed", ("bravo",), closed=True)
    before = output().count("INPUT|KEY|app=1|")
    type_word("ghost"); time.sleep(.2)
    if output().count("INPUT|KEY|app=1|") != before: raise RuntimeError("closed app received input")
    click(950, 700); keys([("right", True), ("right", False)]); type_word("peer")
    wait("INPUT|TEXT|app=2|bravopeer")
    function(6); wait("DYNAMIC|READY|generation=2")
    old_key_count = output().count("INPUT|KEY|app=1|code=44|")
    keys([("z", False)]); time.sleep(.2)
    if output().count("INPUT|KEY|app=1|code=44|") != old_key_count:
        raise RuntimeError("old key release reached a new instance")
    click(150, 700); type_word("fresh")
    wait("INPUT|TEXT|app=1|fresh")
    screen("dynamic-restarted", ("fresh", "bravopeer"), absent=("alphaz",))
    function(7)
    wait("DYNAMIC|FAULT_CONTAINED|generation=2")
    screen("dynamic-faulted", ("bravopeer",), closed=True)
    click(950, 700); keys([("right", True), ("right", False)]); type_word("ok")
    wait("INPUT|TEXT|app=2|bravopeerok")
    function(6)
    peer = "bravopeerok"
    for generation in range(3, 13):
        wait(f"DYNAMIC|READY|generation={generation}")
        click(150, 700); type_word("round" + str(generation))
        wait("INPUT|TEXT|app=1|round" + str(generation))
        function(5); wait(f"DYNAMIC|CLOSED|generation={generation}")
        if generation in (3, 12): screen(f"dynamic-closed-{generation}", ("bravopeerok",), closed=True)
        # Repeated clicks can select the word. Right clears selection at its end.
        click(1000, 700); keys([("right", True), ("right", False)]); type_word("x"); peer += "x"
        wait("INPUT|TEXT|app=2|" + peer)
        function(6)
    wait("DYNAMIC|READY|generation=13")
    wait("DYNAMIC_APP|RPC_DENIALS|generation=13")
    click(150, 700); type_word("clean")
    wait("INPUT|TEXT|app=1|clean")
    screen("dynamic-final", ("clean", "bravopeerok"), absent=("round12", "fresh", "alphaz"))
    (evidence / "lifecycle-result.json").write_text(json.dumps({
        "created_instances": 13, "normal_closes": 11, "contained_faults": 1,
        "checks": ["real Slint process loaded after boot", "separate typed text", "closed window pixels cleared",
                   "closed input denied", "old key release denied to new instance", "peer-surface memory fault contained",
                   "private memory zero on reallocation", "same surface physical memory reused", "all process capabilities revoked",
                   "malformed publication, forged generation and invalid frame rejected",
                   "peer kept its text and accepted input between restarts", "fresh final instance"],
        "observation": "QMP guest framebuffer, OCR and disposable guest diagnostics", "latency_measurement": False,
    }, indent=2) + "\n")
    print("PASS 13 dynamic Slint instances, fault containment, fresh memory/input and surviving peer")
