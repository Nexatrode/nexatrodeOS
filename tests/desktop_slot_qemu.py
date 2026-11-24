#!/usr/bin/env python3
"""Run a disposable, predeclared Microkit slot and two negative controls."""
import argparse
from pathlib import Path
import selectors
import subprocess
import time
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "experiments/desktop-slot"


def build(sdk, output, variant, board_name="qemu_virt_aarch64", configuration="debug"):
    output.mkdir(parents=True, exist_ok=True)
    board = sdk / "board" / board_name / configuration
    for name in ("parent", "child"):
        obj = output / f"{name}.o"
        flags = ["-DSLOT_NEGATIVE_NO_STOP"] if name == "parent" and variant == "no-stop" else []
        subprocess.run(["clang", "--target=aarch64-none-elf", "-mcpu=cortex-a53", "-mgeneral-regs-only",
                        "-std=gnu11", "-O2", "-ffreestanding", "-nostdlib", "-Wall", "-Wextra", "-Werror",
                        "-I", str(board / "include"), *flags, "-c", str(SOURCE / f"{name}.c"), "-o", str(obj)], check=True)
        subprocess.run(["ld.lld", "-L", str(board / "lib"), str(obj), "-lmicrokit", "-T", "microkit.ld",
                        "-o", str(output / f"{name}.elf")], check=True)
    tree = ET.parse(SOURCE / "slot.system")
    if variant == "shared-private":
        child = tree.getroot().find(".//protection_domain[@name='child']")
        ET.SubElement(child, "map", mr="parent_private", vaddr="0x60000000", perms="r")
    system = output / "slot.system"
    tree.write(system, encoding="utf-8", xml_declaration=True)
    subprocess.run([str(sdk / "bin/microkit"), str(system), "--search-path", str(output),
                    "--board", board_name, "--config", configuration,
                    "-o", str(output / "loader.img"), "-r", str(output / "report.txt")], check=True)


def boot(output, expected, forbidden):
    command = ["qemu-system-aarch64", "-machine", "virt,virtualization=on,gic-version=2", "-accel", "tcg",
               "-cpu", "cortex-a53", "-smp", "1", "-m", "2G", "-nic", "none", "-display", "none",
               "-monitor", "none", "-serial", "stdio", "-device", f"loader,file={output / 'loader.img'},addr=0x70000000,cpu-num=0"]
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, bufsize=0)
    data = bytearray()
    try:
        deadline = time.monotonic() + 45
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while time.monotonic() < deadline:
                if not selector.select(timeout=0.2):
                    if process.poll() is not None: break
                    continue
                chunk = process.stdout.read(4096)
                if not chunk: break
                data.extend(chunk)
                if any(marker.encode() in data for marker in forbidden):
                    raise RuntimeError(f"unexpected guest output: {data[-3000:].decode(errors='replace')}")
                if all(marker.encode() in data for marker in expected):
                    (output / "serial.log").write_bytes(data)
                    return
        raise RuntimeError(f"guest did not produce {expected!r}: {data[-3000:].decode(errors='replace')}")
    finally:
        process.terminate()
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill(); process.wait(timeout=5)
        process.stdout.close()


def check_capability_declarations(sdk, output):
    # Test the exact pinned Microkit parser, with a supported extra capability
    # as a positive control. This does not prove that every extension path is
    # impossible; it establishes what this SDF can request without changes.
    for cap, supported in (("cap_tcb", True), ("cap_untyped", False)):
        tree = ET.parse(SOURCE / "slot.system")
        parent = tree.getroot().find("protection_domain[@name='parent']")
        space = ET.SubElement(parent, "cspace")
        attrs = {"slot": "1"}
        if supported: attrs["pd"] = "child"
        ET.SubElement(space, cap, attrs)
        system = output / f"{cap}.system"
        tree.write(system, encoding="utf-8", xml_declaration=True)
        result = subprocess.run([str(sdk / "bin/microkit"), str(system), "--search-path", str(output),
                                 "--board", "qemu_virt_aarch64", "--config", "debug",
                                 "-o", str(output / f"{cap}.img"), "-r", str(output / f"{cap}.report")],
                                text=True, capture_output=True)
        if supported and result.returncode:
            raise RuntimeError("valid TCB capability rejected: " + result.stderr + result.stdout)
        if not supported and (result.returncode == 0 or "Cap type: 'untyped' is not supported" not in result.stderr + result.stdout):
            raise RuntimeError("unexpected untyped capability declaration result: " + result.stderr + result.stdout)
    print("PASS: pinned SDK accepts extra TCB capability and rejects SDF untyped capability", flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sdk", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--utm-build-only", action="store_true", help="build three EL1/SMP loaders for a separate UTM runner")
    args = parser.parse_args()
    sdk, output = args.sdk.resolve(), args.output.resolve()
    if any(item.suffix == ".utm" for item in (output, *output.parents)):
        raise ValueError("output cannot be a VM bundle")
    for variant, expected, forbidden in (
        ("baseline", ["SLOT|PASS|stop-resume", "SLOT|PASS|private-memory-fault"], ["SLOT|FAIL|"]),
        ("no-stop", ["SLOT|FAIL|ran while stopped"], ["SLOT|PASS|stop-resume"]),
        ("shared-private", ["SLOT|PASS|stop-resume", "SLOT|FAIL|private read succeeded"], ["SLOT|PASS|private-memory-fault"]),
    ):
        target = output / variant
        if args.utm_build_only:
            build(sdk, target, variant, "qemu_virt_aarch64_el1", "smp-debug")
            print(f"Built UTM loader: {variant}", flush=True)
        else:
            build(sdk, target, variant)
            boot(target, expected, forbidden)
            print(f"PASS: {variant}", flush=True)
    if not args.utm_build_only:
        check_capability_declarations(sdk, output / "baseline")


if __name__ == "__main__":
    main()
