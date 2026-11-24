#!/usr/bin/env python3
"""Run the bounded dynamic manager alongside the isolated graphical apps."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "experiments/desktop-runtime"
BOARD, CONFIG = "qemu_virt_aarch64_el1", "smp-debug"


def run(*args, **kwargs):
    subprocess.run(list(map(str, args)), check=True, cwd=ROOT, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", required=True, type=Path)
    parser.add_argument("--surfaces", required=True, type=Path)
    parser.add_argument("--composer", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--interactive", action="store_true")
    parser.add_argument("--hvc-sdk", type=Path, help="use this prepared HVC loader for headless HVF checks")
    args = parser.parse_args()
    inputs, surfaces, out = (p.resolve() for p in (args.inputs, args.surfaces, args.output))
    if (out.exists() or out.is_relative_to(inputs) or inputs.is_relative_to(out) or
            any(p.suffix == ".utm" for p in (out, *out.parents))):
        parser.error("choose a fresh staging directory outside VM bundles")
    out.mkdir(parents=True)
    sdk = inputs / "sdk"
    board = sdk / "board" / BOARD / CONFIG
    composer_sdk = sdk
    if args.hvc_sdk:
        composer_sdk = out / "sdk"
        destination = composer_sdk / "board" / BOARD / CONFIG
        (destination / "elf").mkdir(parents=True)
        for entry in board.iterdir():
            if entry.name == "elf":
                for elf in entry.iterdir():
                    target = args.hvc_sdk.resolve() / "board" / BOARD / CONFIG / "elf/loader.elf" if elf.name == "loader.elf" else elf
                    (destination / "elf" / elf.name).symlink_to(target)
            else:
                (destination / entry.name).symlink_to(entry, target_is_directory=entry.is_dir())
    flags = ["clang", "--target=aarch64-none-elf", "-mcpu=cortex-a53", "-mgeneral-regs-only",
             "-std=gnu11", "-O2", "-ffreestanding", "-fno-stack-protector", "-nostdlib",
             "-Wall", "-Wextra", "-Werror", "-I", board / "include"]
    for name in ("app", "composer_placeholder"):
        run(*flags, "-c", RUNTIME / f"{name}.c", "-o", out / f"{name}.o")
    run("ld.lld", "-T", RUNTIME / "app.ld", out / "app.o", "-o", out / "app.elf")
    (out / "payload.S").write_text('.section .rodata\n.balign 8\n.global app_elf_start, app_elf_end\n'
                                   'app_elf_start:\n.incbin "app.elf"\napp_elf_end:\n')
    run(*flags, "-I", out, "-c", out / "payload.S", "-o", out / "payload.o")
    run(*flags, "-DRUNTIME_HOSTED", "-c", RUNTIME / "root.c", "-o", out / "manager.o")
    for name, objects in (("manager", [out / "manager.o", out / "payload.o"]),
                          ("placeholder", [out / "composer_placeholder.o"])):
        run("ld.lld", "-L", board / "lib", *objects, "-lmicrokit", "-T", "microkit.ld",
            "-o", out / f"{name}.elf")

    tree = ET.parse(surfaces / "surfaces.system")
    root = tree.getroot()
    has_input = root.find("protection_domain[@name='input_driver']") is not None
    if has_input != args.interactive:
        parser.error("--interactive must match the supplied graphical artifacts")
    budget = ET.SubElement(root, "protection_domain", name="runtime_budget", priority="1",
                           budget="1000", period="10000")
    ET.SubElement(budget, "program_image", path="placeholder.elf")
    manager = ET.SubElement(root, "protection_domain", name="runtime_manager", priority="105",
                            stack_size="0x10000")
    ET.SubElement(manager, "program_image", path="manager.elf")
    caps = ET.SubElement(manager, "cspace")
    for slot, kind, pd in ((1, "runtime_cnode", "runtime_manager"),
                           (2, "runtime_pool", "runtime_manager"),
                           (3, "runtime_asid", "runtime_manager"),
                           (4, "vspace", "runtime_manager"),
                           (5, "tcb", "runtime_manager"),
                           (6, "sc", "runtime_budget"), (7, "tcb", "runtime_budget")):
        ET.SubElement(caps, "cap_" + kind, slot=str(slot), pd=pd)
    link = ET.SubElement(root, "channel")
    ET.SubElement(link, "end", pd="runtime_manager", id="0", notify="true")
    ET.SubElement(link, "end", pd="compositor", id="5", notify="false")
    tree.write(out / "combined.system", encoding="utf-8", xml_declaration=True)
    run(args.composer.resolve(), out / "combined.system", "--board", BOARD, "--config", CONFIG,
        "--search-path", out, "--search-path", surfaces, "--search-path", surfaces / "gpu",
        "--search-path", inputs / "pds", "--capdl-json", out / "capdl.json",
        "-o", out / "loader.img", "-r", out / "report.txt",
        env=dict(os.environ, MICROKIT_SDK=str(composer_sdk)))
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    identity = {
        "interactive": args.interactive, "cycles": 100,
        "backend": "headless-hvf" if args.hvc_sdk else "utm",
        "loader_sha256": digest(out / "loader.img"),
        "kernel_sha256": digest(board / "elf/sel4.elf"),
        "composer_sha256": digest(args.composer.resolve()),
        "surface_build": json.loads((surfaces / "build.json").read_text()) if (surfaces / "build.json").exists() else None,
        "sources": {str(path.relative_to(ROOT)): digest(path) for path in (
            RUNTIME / "root.c", RUNTIME / "platform.h", RUNTIME / "runtime.h", RUNTIME / "app.c", Path(__file__))},
    }
    (out / "build.json").write_text(json.dumps(identity, indent=2) + "\n")
    if args.build_only:
        return
    if args.hvc_sdk:
        from desktop_input_utm import run_headless
        run_headless(out, args.interactive)
    else:
        run(sys.executable, ROOT / "tests/utm_vault_key_probe.py", "--template", inputs / "template.plist",
            "--loader", out / "loader.img", "--serial-output", out / "serial.log",
            "--screenshot-output", out / "screen.ppm",
            "--expect", "RUNTIME|PASS|complete", "--expect", (
                "SURFACE|PASS|interactive ready" if args.interactive else "SURFACE|PASS|complete"),
            "--reject", "RUNTIME|FAIL|", "--reject", "SURFACE|FAIL|", "--reject", "MON|ERROR",
            "--reject", "panicked at", "--reject", "LDR|ERROR",
            *(["--desktop-input-test"] if args.interactive else []))
    identity["markers"] = [line for line in (out / "serial.log").read_text().splitlines()
                           if "|PASS|" in line]
    identity["input_checks"] = json.loads((out / "input-result.json").read_text()) if args.interactive else None
    (out / "result.json").write_text(json.dumps(identity, indent=2) + "\n")


if __name__ == "__main__":
    main()
