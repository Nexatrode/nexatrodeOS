#!/usr/bin/env python3
"""Create, destroy and recreate one Slint process in a disposable graphical guest."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
BOARD, CONFIG = "qemu_virt_aarch64_el1", "smp-debug"


def run(*args, **kwargs):
    subprocess.run(list(map(str, args)), check=True, cwd=ROOT, **kwargs)


def patch_channels(path):
    """Prepare the pinned Microkit runtime metadata for our fixed fixture ABI."""
    elf = bytearray(path.read_bytes())
    phoff = struct.unpack_from("<Q", elf, 32)[0]
    phsize, phcount = struct.unpack_from("<HH", elf, 54)
    assert elf[:6] == b"\x7fELF\x02\x01" and phsize == 56 and phcount <= 8
    programs = [struct.unpack_from("<IIQQQQQQ", elf, phoff + i * phsize) for i in range(phcount)]
    symbols = subprocess.check_output(["/opt/homebrew/opt/llvm/bin/llvm-nm", "--format=posix", str(path)], text=True)
    locations = {row[0]: (int(row[2], 16), int(row[3], 16)) for line in symbols.splitlines()
                 if len(row := line.split()) == 4 and row[0] in ("microkit_pps", "microkit_name")}
    for name, value in (("microkit_pps", struct.pack("<Q", 1)), ("microkit_name", b"dynamic_notes")):
        address, size = locations[name]
        if (name == "microkit_pps" and size != 8) or len(value) > size: raise ValueError("unexpected runtime symbol")
        for kind, flags, offset, vaddr, _, filesz, _, _ in programs:
            if kind == 1 and flags & 2 and vaddr <= address and address + size <= vaddr + filesz:
                start = offset + address - vaddr
                elf[start:start + size] = value.ljust(size, b"\0")
                break
        else: raise ValueError("runtime metadata is outside writable file data")
    path.write_bytes(elf)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", required=True, type=Path)
    parser.add_argument("--surfaces", required=True, type=Path)
    parser.add_argument("--composer", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--hvc-sdk", type=Path)
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--control-skip-revoke", action="store_true")
    parser.add_argument("--control-keep-endpoint", action="store_true")
    args = parser.parse_args()
    if args.control_skip_revoke and args.control_keep_endpoint: parser.error("choose one negative control")
    mode = "skip-revoke" if args.control_skip_revoke else "keep-endpoint" if args.control_keep_endpoint else "normal"
    inputs, surfaces, out = (p.resolve() for p in (args.inputs, args.surfaces, args.output))
    if (out.exists() or out.is_relative_to(inputs) or inputs.is_relative_to(out) or
            any(p.suffix == ".utm" for p in (out, *out.parents))):
        parser.error("choose a fresh staging directory outside VM bundles and prepared inputs")
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

    target = ROOT / "build/desktop-graphical-cargo"
    env = dict(os.environ, SEL4_PREFIX=str(inputs / "sel4-install"),
               SEL4_INCLUDE_DIRS=str(board / "include"), CARGO_TARGET_DIR=str(target),
               PATH="/opt/homebrew/opt/llvm/bin:" + os.environ["PATH"])
    run("cargo", "+nightly-2026-09-25", "build", "--locked", "--offline", "--release", "-Zjson-target-spec",
        "--config", ROOT / "rust/.cargo/config.toml", "--manifest-path", ROOT / "experiments/slint-notes/Cargo.toml",
        "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json", "--features", "desktop-dynamic-ui",
        "--bin", "slint_surface_fixture", env=env)
    (out / "identity.bin").write_bytes(struct.pack("<QQ", 1, 1))
    run("/opt/homebrew/opt/llvm/bin/llvm-objcopy", "--update-section", f".app_identity={out / 'identity.bin'}",
        target / "aarch64-sel4-microkit/release/slint_surface_fixture.elf", out / "gui.elf")
    patch_channels(out / "gui.elf")
    flags = ["clang", "--target=aarch64-none-elf", "-mcpu=cortex-a53", "-mgeneral-regs-only",
             "-std=gnu11", "-O2", "-ffreestanding", "-fno-stack-protector", "-nostdlib",
             "-Wall", "-Wextra", "-Werror", "-I", board / "include"]
    runtime = ROOT / "experiments/desktop-runtime"
    for name in ("app", "composer_placeholder"):
        run(*flags, "-c", runtime / f"{name}.c", "-o", out / f"{name}.o")
    run("ld.lld", "-T", runtime / "app.ld", out / "app.o", "-o", out / "app.elf")
    (out / "payload.S").write_text(''.join(
        f'.section .rodata\n.balign 8\n.global {name}_elf_start, {name}_elf_end\n'
        f'{name}_elf_start:\n.incbin "{name}.elf"\n{name}_elf_end:\n' for name in ("app", "gui")))
    run(*flags, "-I", out, "-c", out / "payload.S", "-o", out / "payload.o")
    run(*flags, *(["-DCONTROL_SKIP_REVOKE"] if args.control_skip_revoke else []),
        *(["-DCONTROL_KEEP_ENDPOINT"] if args.control_keep_endpoint else []), "-c",
        ROOT / "experiments/desktop-graphical/manager.c", "-o", out / "manager.o")
    for name, objects in (("manager", [out / "manager.o", out / "payload.o"]),
                          ("placeholder", [out / "composer_placeholder.o"])):
        run("ld.lld", "-L", board / "lib", *objects, "-lmicrokit", "-T", "microkit.ld", "-o", out / f"{name}.elf")

    tree = ET.parse(surfaces / "surfaces.system")
    system = tree.getroot()
    manager = system.find("protection_domain[@name='compositor']")
    if manager is None or system.find("protection_domain[@name='input_driver']") is None:
        parser.error("supply interactive surface artifacts")
    for child in list(manager.findall("protection_domain")):
        manager.remove(child)
        if child.get("name") == "app_2":
            child.attrib.pop("id")
            system.append(child)
    for mapping in list(manager.findall("map")):
        if mapping.get("mr") in ("surface_1", "app_input_1"):
            manager.remove(mapping)
    manager.set("name", "runtime_manager")
    manager.set("priority", "99")
    manager.set("budget", "8000")
    manager.set("period", "10000")
    manager.find("program_image").set("path", "manager.elf")
    for link in list(system.findall("channel")):
        if any(end.get("pd") == "app_1" for end in link):
            system.remove(link)
            continue
        for end in link:
            if end.get("pd") == "compositor": end.set("pd", "runtime_manager")
    budget = ET.SubElement(manager, "protection_domain", name="runtime_budget", id="0", priority="1", budget="2500", period="10000")
    ET.SubElement(budget, "program_image", path="placeholder.elf")
    caps = ET.SubElement(manager, "cspace")
    for slot, kind, pd in ((1, "runtime_cnode", "runtime_manager"), (2, "runtime_pool", "runtime_manager"),
                           (3, "runtime_asid", "runtime_manager"), (4, "vspace", "runtime_manager"),
                           (5, "tcb", "runtime_manager"), (6, "sc", "runtime_budget"), (7, "tcb", "runtime_budget")):
        ET.SubElement(caps, "cap_" + kind, slot=str(slot), pd=pd)
    used = {m.get("mr") for m in system.iter("map")}
    for mr in list(system.findall("memory_region")):
        if mr.get("name") not in used: system.remove(mr)
    tree.write(out / "graphical.system", encoding="utf-8", xml_declaration=True)
    run(args.composer.resolve(), out / "graphical.system", "--board", BOARD, "--config", CONFIG,
        "--search-path", out, "--search-path", surfaces, "--search-path", surfaces / "gpu", "--search-path", inputs / "pds",
        "-o", out / "loader.img", "-r", out / "report.txt", env=dict(os.environ, MICROKIT_SDK=str(composer_sdk)))
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    identity = {"backend": "headless-hvf" if args.hvc_sdk else "utm", "control": mode,
                "loader_sha256": digest(out / "loader.img"), "kernel_sha256": digest(board / "elf/sel4.elf"),
                "composer_sha256": digest(args.composer.resolve()), "gui_sha256": digest(out / "gui.elf"),
                "sources": {str(p.relative_to(ROOT)): digest(p) for p in (
                    ROOT / "experiments/desktop-graphical/manager.c", ROOT / "experiments/desktop-graphical/image.h",
                    ROOT / "experiments/desktop-surfaces/compositor.c", ROOT / "experiments/desktop-surfaces/app.rs", Path(__file__).resolve())}}
    (out / "build.json").write_text(json.dumps(identity, indent=2) + "\n")
    if args.build_only: return
    if args.hvc_sdk:
        from desktop_input_utm import run_headless
        from desktop_graphical_input import exercise
        run_headless(out, False, expected=("DYNAMIC|READY|generation=1", "SURFACE|PASS|interactive ready"),
                     interaction=lambda *a: exercise(*a, control=None if mode == "normal" else mode))
    else:
        run(sys.executable, ROOT / "tests/utm_vault_key_probe.py", "--template", inputs / "template.plist",
            "--loader", out / "loader.img", "--serial-output", out / "serial.log", "--screenshot-output", out / "screen.ppm",
            "--expect", "DYNAMIC|READY|generation=1", "--expect", "SURFACE|PASS|interactive ready",
            "--reject", "RUNTIME|FAIL|", "--reject", "SURFACE|FAIL|", "--reject", "MON|ERROR", "--reject", "panicked at",
            "--desktop-lifecycle-test", mode)
    identity["checks"] = json.loads((out / "lifecycle-result.json").read_text())
    (out / "result.json").write_text(json.dumps(identity, indent=2) + "\n")


if __name__ == "__main__": main()
