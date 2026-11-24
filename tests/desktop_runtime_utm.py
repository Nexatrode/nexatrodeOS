#!/usr/bin/env python3
"""Build a dedicated seL4 process-manager root task and test it in disposable UTM."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "experiments/desktop-runtime"
BOARD = "qemu_virt_aarch64_el1"
CONFIG = "smp-debug"


def run(*arguments, **kwargs):
    subprocess.run(list(map(str, arguments)), check=True, cwd=ROOT, **kwargs)

def run_headless(loader, output, expected):
    """Use the same four-core kernel through Homebrew QEMU's HVC conduit."""
    serial = output / "serial.log"
    command = ["qemu-system-aarch64", "-machine",
               "virt,virtualization=off,secure=off,gic-version=3,kernel-irqchip=on",
               "-accel", "hvf", "-cpu", "host", "-smp", "4", "-m", "2048",
               "-nodefaults", "-display", "none", "-monitor", "none",
               "-serial", f"file:{serial}", "-device",
               f"loader,file={loader},addr=0x70000000,cpu-num=0"]
    with (output / "qemu.log").open("w") as errors:
        process = subprocess.Popen(command, stdout=errors, stderr=errors)
        try:
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline:
                text = serial.read_text() if serial.exists() else ""
                if expected in text:
                    return
                if "RUNTIME|FAIL|" in text or process.poll() is not None:
                    raise RuntimeError("headless guest failed: " + text[-3000:])
                time.sleep(0.1)
            raise RuntimeError("headless guest timed out: " + text[-3000:])
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--control", choices=["skip-revoke", "keep-escaped"])
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--hvc-sdk", type=Path, help="run headless HVF using this prepared SDK's HVC loader")
    args = parser.parse_args()
    inputs, output = args.inputs.resolve(), args.output.resolve()
    if (any(p.suffix == ".utm" for p in (output, *output.parents)) or
            output.is_relative_to(inputs) or inputs.is_relative_to(output)):
        raise ValueError("output must be a separate build directory")
    output.mkdir(parents=True, exist_ok=True)
    sdk = inputs / "sdk"
    board = sdk / "board" / BOARD / CONFIG
    flags = ["clang", "--target=aarch64-none-elf", "-mcpu=cortex-a53", "-mgeneral-regs-only",
             "-std=gnu11", "-O2", "-ffreestanding", "-fno-stack-protector", "-nostdlib",
             "-Wall", "-Wextra", "-Werror", "-I", board / "include"]
    for name in ("app", "root"):
        control = ["-DCONTROL_" + args.control.replace("-", "_").upper()] if args.control and name == "root" else []
        run(*flags, *control, "-c", SOURCE / f"{name}.c", "-o", output / f"{name}.o")
    run(*flags, "-c", SOURCE / "composer_placeholder.c", "-o", output / "placeholder.o")
    run("ld.lld", "-L", board / "lib", "-T", "microkit.ld", output / "placeholder.o",
        "-lmicrokit", "-o", output / "placeholder.elf")
    run(*flags, "-c", SOURCE / "start.S", "-o", output / "start.o")
    run("ld.lld", "-T", SOURCE / "app.ld", output / "app.o", "-o", output / "app.elf")
    (output / "payload.S").write_text('.section .rodata\n.balign 8\n.global app_elf_start, app_elf_end\n'
                                      'app_elf_start:\n.incbin "app.elf"\napp_elf_end:\n')
    run(*flags, "-I", output, "-c", output / "payload.S", "-o", output / "payload.o")
    run("ld.lld", "-T", SOURCE / "root.ld", output / "root.o", output / "start.o",
        output / "payload.o", "-o", output / "root.elf")
    # Use the known kernel and loader with our own initial root task. The
    # composer's generated Microkit CapDL spec is deliberately unused.
    overlay = output / "sdk"
    destination = overlay / "board" / BOARD / CONFIG
    (destination / "elf").mkdir(parents=True, exist_ok=True)
    for entry in board.iterdir():
        if entry.name == "elf":
            for elf in entry.iterdir():
                target = destination / "elf" / elf.name
                if elf.name == "loader.elf" and args.hvc_sdk:
                    elf = args.hvc_sdk.resolve() / "board" / BOARD / CONFIG / "elf/loader.elf"
                if target.is_symlink() and target.resolve() != elf.resolve():
                    target.unlink()
                if elf.name == "initialiser.elf":
                    if target.is_symlink():
                        raise ValueError("initial root output must be a private regular file")
                    shutil.copyfile(output / "root.elf", target)
                elif not target.exists():
                    target.symlink_to(elf)
        elif not (destination / entry.name).exists():
            (destination / entry.name).symlink_to(entry, target_is_directory=entry.is_dir())
    system = output / "bootstrap.system"
    system.write_text('<system><protection_domain name="unused_specification" priority="100">'
                      '<program_image path="placeholder.elf"/></protection_domain></system>\n')
    env = dict(os.environ, MICROKIT_SDK=str(overlay))
    run(sdk / "bin/microkit", system, "--board", BOARD, "--config", CONFIG,
        "--search-path", output, "-o", output / "loader.img", "-r", output / "report.txt", env=env)
    if args.build_only:
        print("Built dedicated root-task loader", output / "loader.img")
        return
    expected = {"skip-revoke": "RUNTIME|FAIL|object capability survived",
                "keep-escaped": "RUNTIME|FAIL|escaped capability survived"}.get(args.control)
    if args.hvc_sdk:
        run_headless(output / "loader.img", output, expected or "RUNTIME|PASS|complete")
    else:
        run(sys.executable, ROOT / "tests/utm_vault_key_probe.py",
            "--template", inputs / "template.plist", "--loader", output / "loader.img",
            "--serial-output", output / "serial.log",
            "--expect", expected or "RUNTIME|PASS|complete",
            "--reject", "RUNTIME|PASS|complete" if expected else "RUNTIME|FAIL|",
            "--reject", "Kernel panic")
    trace = (output / "serial.log").read_text()
    failures = [line.strip() for line in trace.splitlines() if "RUNTIME|FAIL|" in line]
    if failures != ([expected + " error=0"] if expected else []):
        raise RuntimeError(f"unexpected runtime failure markers: {failures}")
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    (output / "result.json").write_text(json.dumps({
        "control": args.control,
        "backend": "headless-hvf" if args.hvc_sdk else "utm",
        "cycles": 100 if not expected else 0,
        "loader_sha256": digest(output / "loader.img"),
        "kernel_sha256": digest(board / "elf/sel4.elf"),
        "sources": {path.name: digest(path) for path in SOURCE.iterdir() if path.is_file()},
        "markers": [line.strip() for line in trace.splitlines() if "RUNTIME|" in line],
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()
