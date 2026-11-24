#!/usr/bin/env python3
"""Present two isolated Slint app surfaces through the existing UTM display path."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "experiments/desktop-surfaces"
BOARD, CONFIG = "qemu_virt_aarch64_el1", "smp-debug"


def run(*arguments, **kwargs):
    subprocess.run(list(map(str, arguments)), cwd=ROOT, check=True, **kwargs)


def map_region(pd, name, address, permissions, symbol=None):
    attrs = dict(mr=name, vaddr=hex(address), perms=permissions, cached="true")
    if symbol:
        attrs["setvar_vaddr"] = symbol
    ET.SubElement(pd, "map", attrs)


def channel(root, first, second, both=False):
    link = ET.SubElement(root, "channel")
    ET.SubElement(link, "end", pd=first[0], id=str(first[1]), notify="true")
    ET.SubElement(link, "end", pd=second[0], id=str(second[1]), notify="true" if both else "false")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", required=True, type=Path)
    parser.add_argument("--sddf", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--control-share-peer", action="store_true")
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--interactive", action="store_true", help="enable isolated input queues and focus")
    args = parser.parse_args()
    inputs, out, sddf = (p.resolve() for p in (args.inputs, args.output, args.sddf))
    if out.exists() or out.is_relative_to(inputs) or out.is_relative_to(sddf):
        parser.error("choose a new output directory")
    if any(p.suffix == ".utm" for p in (out, *out.parents)):
        parser.error("output cannot be a VM bundle")
    out.mkdir(parents=True)
    sdk, board = inputs / "sdk", inputs / "sdk/board" / BOARD / CONFIG
    env = dict(os.environ, SEL4_PREFIX=str(inputs / "sel4-install"),
               SEL4_INCLUDE_DIRS=str(board / "include"), CARGO_TARGET_DIR=str(ROOT / "build/desktop-surfaces-cargo"),
               PATH="/opt/homebrew/opt/llvm/bin:" + os.environ["PATH"])
    run("cargo", "+nightly-2026-09-25", "build", "--locked", "--offline", "--release",
        "-Zjson-target-spec", "--config", ROOT / "rust/.cargo/config.toml",
        "--manifest-path", ROOT / "experiments/slint-notes/Cargo.toml",
        "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json", "--features", "utm-pd",
        "--bin", "slint_surface_fixture", env=env)
    ui = ROOT / "build/desktop-surfaces-cargo/aarch64-sel4-microkit/release"
    run("clang", "--target=aarch64-none-elf", "-mcpu=cortex-a53", "-mgeneral-regs-only",
        "-std=gnu11", "-O2", "-ffreestanding", "-nostdlib", "-Wall", "-Wextra", "-Werror",
        *(["-DSURFACE_INTERACTIVE"] if args.interactive else []),
        "-I", board / "include", "-c", SOURCE / "compositor.c", "-o", out / "compositor.o")
    run("ld.lld", "-L", board / "lib", out / "compositor.o", "-lmicrokit", "-T", "microkit.ld",
        "-o", out / "compositor.elf")
    os.environ["NEXATRODE_SDDF"] = str(sddf)
    spec = importlib.util.spec_from_file_location("accelerated", ROOT / "experiments/hvf-smp/accelerated.py")
    accelerated = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(accelerated)
    gpu = accelerated.build_gpu_from_source(inputs, out)
    if args.interactive:
        run("clang", "--target=aarch64-none-elf", "-mcpu=cortex-a53", "-mstrict-align",
            "-std=gnu17", "-O2", "-ffreestanding", "-nostdlib", "-Wall", "-Wextra", "-Werror",
            "-I", board / "include", "-I", sddf / "include", "-I", sddf / "include/microkit",
            "-I", sddf / "include/sddf/util/custom_libc", "-c", ROOT / "tests/input/driver.c",
            "-o", out / "input_driver.o")
        run("ld.lld", "-L", board / "lib", "-L", gpu, out / "input_driver.o",
            "--start-group", "-lmicrokit", "-lsddf_util_debug", "--end-group", "-T", "microkit.ld",
            "-o", out / "input_driver.elf")

    tree = ET.parse(ROOT / "experiments/hvf-smp/accelerated.system")
    root = tree.getroot()
    keep = {"timer_driver", "gpu_driver", "gpu_virt", "gpu_client"}
    if args.interactive:
        keep.add("input_driver")
    for pd in root.findall("protection_domain"):
        if pd.get("name") not in keep:
            root.remove(pd)
    for link in root.findall("channel"):
        if any(end.get("pd") not in keep for end in link):
            root.remove(link)
    pd = ET.SubElement(root, "protection_domain", name="compositor", priority="99", stack_size="0x10000")
    ET.SubElement(pd, "program_image", path="compositor.elf")
    map_region(pd, "slint_frame", 0x62000000, "rw", "output_frame")
    map_region(pd, "ui_focus", 0x63000000, "rw", "focus_page")
    for identity in (1, 2):
        surface, heap = f"surface_{identity}", f"heap_{identity}"
        root.insert(0, ET.Element("memory_region", name=surface, size="0x200000", page_size="0x200000"))
        root.insert(0, ET.Element("memory_region", name=heap, size="0x800000", page_size="0x200000"))
        map_region(pd, surface, 0x60000000 + (identity - 1) * 0x1000000, "r",
                   "surface_a" if identity == 1 else "surface_b")
        app = ET.SubElement(pd, "protection_domain", name=f"app_{identity}", id=str(identity),
                            priority="100", budget="2500", period="10000", stack_size="0x20000")
        image_name = f"app_{identity}.elf"
        config = out / f"identity_{identity}.bin"
        config.write_bytes(struct.pack("<QQ", identity, int(args.interactive)))
        run("/opt/homebrew/opt/llvm/bin/llvm-objcopy", "--update-section",
            f".app_identity={config}", ui / "slint_surface_fixture.elf", out / image_name)
        ET.SubElement(app, "program_image", path=image_name)
        map_region(app, surface, 0x60000000, "rw", "app_surface")
        map_region(app, heap, 0x64000000, "rw", "app_heap")
        if args.interactive:
            queue = f"app_input_{identity}"
            root.insert(0, ET.Element("memory_region", name=queue, size="0x1000"))
            map_region(pd, queue, 0x66000000 + identity * 0x1000, "rw",
                       "input_a" if identity == 1 else "input_b")
            map_region(app, queue, 0x66000000, "rw", "app_input")
            channel(root, ("compositor", identity + 6), (f"app_{identity}", 2))
        if args.control_share_peer and identity == 1:
            map_region(app, "surface_2", 0x61000000, "r")
        channel(root, ("compositor", identity - 1), (f"app_{identity}", 0), both=True)
    channel(root, ("compositor", 2), ("gpu_client", 2))
    channel(root, ("compositor", 3), ("app_1", 1))
    channel(root, ("timer_driver", 0), ("compositor", 4))
    if args.interactive:
        map_region(pd, "input_handoff", 0x67000000, "rw", "input_handoff")
        channel(root, ("input_driver", 2), ("compositor", 6))
    used = {m.get("mr") for m in root.iter("map")}
    for mr in root.findall("memory_region"):
        if mr.get("name") not in used:
            root.remove(mr)
    tree.write(out / "surfaces.system", encoding="utf-8", xml_declaration=True)
    run(sdk / "bin/microkit", out / "surfaces.system", "--search-path", out,
        "--search-path", ui, "--search-path", gpu, "--search-path", inputs / "pds",
        "--board", BOARD, "--config", CONFIG, "-o", out / "loader.img", "-r", out / "report.txt")
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    (out / "build.json").write_text(json.dumps({
        "interactive": args.interactive, "shared_peer_control": args.control_share_peer,
        "loader_sha256": digest(out / "loader.img"),
        "kernel_sha256": digest(board / "elf/sel4.elf"),
        "sources": {str(path.relative_to(ROOT)): digest(path) for path in (
            SOURCE / "app.rs", SOURCE / "compositor.c", SOURCE / "input.h", Path(__file__).resolve(),
            ROOT / "experiments/slint-notes/ui/notes.slint", ROOT / "tests/input/driver.c")},
        "artifacts": {name: digest(out / name) for name in ("app_1.elf", "app_2.elf", "compositor.elf")},
    }, indent=2) + "\n")
    if args.build_only:
        print("Built isolated Slint surface loader", out / "loader.img")
        return
    if args.interactive and args.control_share_peer:
        parser.error("use the scripted fixture for the shared-peer control")
    target = "SURFACE|FAIL|peer surface readable" if args.control_share_peer else (
        "SURFACE|PASS|interactive ready" if args.interactive else "SURFACE|PASS|complete")
    reject = "SURFACE|PASS|complete" if args.control_share_peer else "SURFACE|FAIL|"
    capture = [] if args.control_share_peer else ["--screenshot-output", out / "screen.ppm"]
    run(sys.executable, ROOT / "tests/utm_vault_key_probe.py", "--template", inputs / "template.plist",
        "--loader", out / "loader.img", "--serial-output", out / "serial.log",
        "--expect", target, "--reject", reject, "--reject", "MON|ERROR", *capture,
        *(["--desktop-input-test"] if args.interactive else []))
    if not args.control_share_peer and not args.interactive:
        signature, dimensions, maximum, pixels = (out / "screen.ppm").read_bytes().split(b"\n", 3)
        if (signature, dimensions, maximum) != (b"P6", b"1080 2340", b"255"):
            raise RuntimeError("unexpected framebuffer format")
        if len(pixels) != 1080 * 2340 * 3:
            raise RuntimeError("incomplete framebuffer")
        for x, y, expected in ((10, 10, (4, 12, 22)),
                               (50, 510, (8, 21, 34)), (570, 510, (8, 21, 34)),
                               (450, 1210, (104, 221, 194)), (950, 1210, (104, 221, 194))):
            offset = (y * 1080 + x) * 3
            if tuple(pixels[offset:offset + 3]) != expected:
                raise RuntimeError(f"composited framebuffer pixel mismatch at {x}, {y}")
        print("PASS: both Slint windows reached the guest framebuffer")


if __name__ == "__main__":
    main()
