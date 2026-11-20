#!/usr/bin/env python3
"""Run the raw loader's relocation/MMU path in QEMU, without phone access.

This checks generic ARM execution, not Apple A10 cache or display behavior.
The 36 GiB guest address span preserves the image's >32 GiB physical addresses;
QEMU demand-pages RAM. No guest disk or network device is used.
--fault injects SVC into a temporary copy, leaving the upload artifact intact.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import struct
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("elf", type=Path)
    parser.add_argument("payload", type=Path)
    parser.add_argument("--fault", action="store_true")
    parser.add_argument("--expect-exception", action="store_true",
                        help="expect an exception from the input image without modifying it")
    parser.add_argument("--register-report", type=Path,
                        help="manifest for the terminal pre-MMU numeric report")
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    llvm = Path(os.environ.get("LLVM_BIN", "/opt/homebrew/opt/llvm/bin"))
    symbols = {}
    for line in subprocess.check_output([str(llvm / "llvm-nm"), str(args.elf)], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            symbols[fields[2]] = int(fields[0], 16)
    original = args.payload.read_bytes()
    numeric = json.loads(args.register_report.read_text()) if args.register_report else None
    transition16 = numeric and numeric.get("mode") == "16k-transition"
    if numeric and (args.fault or args.expect_exception or
                    numeric["sha256"] != hashlib.sha256(original).hexdigest()):
        raise SystemExit("Numeric report requires matching manifest and no exception options")
    image = bytearray(original)
    base = symbols["_start"]
    if args.fault:
        struct.pack_into("<I", image, symbols["el1_mmu_enable"] - base, 0xd4000001)
    expect_exception = args.fault or args.expect_exception
    target = int(numeric["report_wait"], 16) if numeric else symbols[
        "pongo_mmu_fault_vector" if expect_exception else "start_kernel"]
    # WFE is followed by a backwards branch in both terminal paths.
    waits = [target+i for i in range(0, 4 if numeric else 128, 4)
             if struct.unpack_from("<I", original, target-base+i)[0] == 0xd503205f]
    if len(waits) != 1:
        raise SystemExit("Could not identify a unique expected terminal wait")
    args.report.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="iphone-loader-") as directory:
        tmp = Path(directory)
        (tmp / "payload.bin").write_bytes(image)
        # Set X0 to synthetic boot_args and branch to Pongo's observed receive
        # address. The real loader then relocates itself to 0x810000000.
        (tmp / "entry.bin").write_bytes(struct.pack(
            "<IIIIQQ", 0x58000080, 0x580000a1, 0xd61f0020, 0xd503201f,
            0x41000000, 0x8BADC0000))
        bootargs = bytearray(256)
        for offset, value in {0x28: 0x8BB80C000, 0x38: 0x1100, 0x40: 1080, 0x48: 1920}.items():
            struct.pack_into("<Q", bootargs, offset, value)
        (tmp / "args.bin").write_bytes(bootargs)
        (tmp / "framebuffer.bin").write_bytes(b"\xff" * (0x1100 * 1920))
        address = str(tmp / "qmp.sock")
        command = ["qemu-system-aarch64", "-machine", "virt,virtualization=off",
                   "-cpu", "max" if transition16 else "cortex-a72", "-accel", "tcg", "-m", "36G", "-S",
                   "-display", "none", "-serial", "none", "-nic", "none",
                   "-qmp", "unix:" + address + ",server=on,wait=off"]
        for name, location in (("entry", 0x40200000), ("args", 0x41000000),
                               ("framebuffer", 0x8BB80C000), ("payload", 0x8BADC0000)):
            device = f"loader,file={tmp}/{name}.bin,addr={location:#x},force-raw=on"
            if name == "entry":
                device += ",cpu-num=0"
            command += ["-device", device]
        with (tmp / "stderr").open("w") as err:
            proc = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=err)
            try:
                for _ in range(50):
                    if proc.poll() is not None:
                        raise RuntimeError((tmp / "stderr").read_text())
                    if Path(address).exists():
                        break
                    time.sleep(0.1)
                with socket.socket(socket.AF_UNIX) as connection:
                    connection.settimeout(5)
                    connection.connect(address)
                    stream = connection.makefile("rwb")
                    json.loads(stream.readline())  # QMP greeting

                    def qmp(execute, arguments=None):
                        message = {"execute": execute}
                        if arguments is not None:
                            message["arguments"] = arguments
                        stream.write(json.dumps(message).encode() + b"\n")
                        stream.flush()
                        while True:
                            result = json.loads(stream.readline())
                            if "error" in result:
                                raise RuntimeError(result["error"])
                            if "return" in result:
                                return result["return"]

                    def hmp(text):
                        return qmp("human-monitor-command", {"command-line": text})

                    qmp("qmp_capabilities")
                    qmp("cont")
                    time.sleep(2)
                    qmp("stop")
                    registers = hmp("info registers")
                    pc = int(re.search(r"PC=([0-9a-fA-F]+)", registers)[1], 16)

                    def pixel(x, y):
                        text = hmp(f"xp /1wx {0x8BB80C000 + y * 0x1100 + x * 4:#x}")
                        return int(text.split(":", 1)[1].strip(), 16)

                    samples = {"origin": pixel(0, 0), "square_end": pixel(127, 127),
                               "outside_square": pixel(128, 127), "mmu_band": pixel(0, 280)}
                    passed = pc in (waits[0], waits[0] + 4)
                    numeric_values = None
                    if numeric:
                        hmp(f'pmemsave {numeric["report_values"]} 128 "{tmp}/values.bin"')
                        values = struct.unpack("<16Q", (tmp / "values.bin").read_bytes())
                        expected = {0: 0x004d4d5550524531, 1: 4,
                                    3: 0x10b0903019 | ((values[8] & 7) << 32),
                                    4: 0x810009000, 5: 0x810005000, 6: 0xff440c0400,
                                    7: 0x810002000, 10: 0x30d00801,
                                    11: 0x800000711, 12: 0x880000601,
                                    13: 0x800000711, 14: 0x880000601}
                        if transition16:
                            expected = {0: 0x0031364b50415353, 1: 4, 2: 0x30d00801,
                                        3: 0x709cb01c | ((values[8] & 7) << 32),
                                        4: 0x810004000, 5: 0x810004000, 6: 0xff440c0400,
                                        7: 0x810002000, 10: 0x30d00801,
                                        11: 0x810000711, 12: 0x8ba000601, 13: 0x8bc000601}
                            passed &= (values[14] & 0x0000fffffffff001) == 0x810001000
                            passed &= (values[15] & 0x0000fffffffff001) == 0x8bb80c000
                        passed &= all(values[i] == value for i, value in expected.items())
                        passed &= values[2] & 0x1005 == (1 if transition16 else 0)
                        passed &= 0x810003000 <= values[9] < 0x810004000
                        hmp(f'pmemsave 0x8bb80c000 {0x1100*784} "{tmp}/screen.bin"')
                        screen = (tmp / "screen.bin").read_bytes()
                        # Independently decode all displayed glyphs from framebuffer
                        # pixels and compare them to the captured register values.
                        font = ["75557", "26227", "71747", "71717", "55711", "74717",
                                "74757", "71111", "75757", "75717", "75755", "65656",
                                "74447", "65556", "74647", "74644"]
                        decoded = []
                        for row in range(16):
                            digits = ""
                            for glyph in range(17):
                                x = 8 if glyph == 0 else 16 + glyph * 32
                                pattern = ""
                                for y in range(5):
                                    bits = 0
                                    for column in range(3):
                                        offset = (8+row*48+y*8+4)*0x1100 + (x+column*8+4)*4
                                        pixel_value, = struct.unpack_from("<I", screen, offset)
                                        if pixel_value not in (0, 0xffffffff):
                                            raise RuntimeError("Report glyph has non-monochrome pixel")
                                        bits = (bits << 1) | int(pixel_value == 0xffffffff)
                                    pattern += str(bits)
                                digits += format(font.index(pattern), "x")
                            passed &= digits == f"{row:x}{values[row]:016x}"
                            decoded.append(digits)
                        numeric_values = dict(zip(numeric["rows"], map(hex, values)))
                        args.report.with_suffix(".pixels").write_bytes(screen)
                    elif expect_exception:
                        passed &= samples["origin"] == 0 and samples["square_end"] == 0
                        passed &= samples["outside_square"] == 0xffffffff
                    else:
                        passed &= samples["origin"] == 0xff000000 and samples["mmu_band"] == 0
                    report = {"passed": passed, "case": "register-report" if numeric else
                              "exception" if expect_exception else "mmu-return",
                              "injected_fault_in_emulator": args.fault,
                              "payload_sha256": hashlib.sha256(original).hexdigest(),
                              "pc": hex(pc), "expected_wait": hex(waits[0]),
                              "pixels": {key: hex(value) for key, value in samples.items()},
                              "registers": registers,
                              "tables": hmp("xp /4gx 0x810009100"),
                              "limitation": "Generic QEMU max CPU emulation; not Apple A10 or USB/display hardware."
                              if transition16 else "Generic Cortex-A72 emulation; does not model Apple A10 or USB/display hardware."}
                    if numeric:
                        report["numeric_values"] = numeric_values
                        report["decoded_display_rows"] = decoded
                    args.report.write_text(json.dumps(report, indent=2) + "\n")
                    print(f"{report['case']}: {'PASS' if passed else 'FAIL'}, PC={pc:#x}; report={args.report}")
                    if not passed:
                        raise SystemExit(1)
            finally:
                proc.terminate()
                try:
                    proc.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()


if __name__ == "__main__":
    main()
