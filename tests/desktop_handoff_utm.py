#!/usr/bin/env python3
"""Run a native app handoff with a blank disk and passphrase-unlocked test vault."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "experiments/desktop-handoff"
BOARD = "qemu_virt_aarch64_el1"
CONFIG = "smp-debug"


def run(*args, **kwargs):
    subprocess.run(list(map(str, args)), cwd=ROOT, check=True, **kwargs)


def domain(parent, name, elf, priority, **attrs):
    pd = ET.SubElement(parent, "protection_domain", name=name, priority=str(priority), **attrs)
    ET.SubElement(pd, "program_image", path=elf)
    return pd


def mapping(pd, name, addr, perms, symbol=None, **attrs):
    if symbol:
        attrs["setvar_vaddr"] = symbol
    ET.SubElement(pd, "map", mr=name, vaddr=hex(addr), perms=perms, **attrs)


def channel(root, a, b, pp=False, bidirectional=False):
    link = ET.SubElement(root, "channel")
    ET.SubElement(link, "end", pd=a[0], id=str(a[1]), notify="false" if pp else "true",
                  pp="true" if pp else "false")
    ET.SubElement(link, "end", pd=b[0], id=str(b[1]), notify="true" if bidirectional else "false")


def system(storage, keep_identity):
    tree = ET.parse(storage / "storage.system")
    root = tree.getroot()
    keep = {"blk_driver", "blk_virt", "vault"}
    for pd in root.findall("protection_domain"):
        if pd.get("name") not in keep:
            root.remove(pd)
    for link in root.findall("channel"):
        if any(end.get("pd") not in keep for end in link):
            root.remove(link)
    used = {m.get("mr") for m in root.iter("map")}
    for mr in root.findall("memory_region"):
        if mr.get("name") not in used:
            root.remove(mr)
    vault = root.find("protection_domain[@name='vault']")
    vault.set("stack_size", "0x80000")
    for name, size in (("device_lock_transfer", 4096), ("image", 4096), ("scratch", 16384),
                       ("chess_launch", 4096), ("next_launch", 4096)):
        root.insert(0, ET.Element("memory_region", name=name, size=hex(size)))
    mapping(vault, "device_lock_transfer", 0x4f000000, "rw", "device_lock_transfer")
    wallet = domain(root, "test_wallet", "desktop_test_wallet.elf", 99, stack_size="0x40000")
    mapping(wallet, "device_lock_transfer", 0x36000000, "rw", "lock_page")
    mapping(wallet, "wallet_transfer", 0x34000000, "rw", "wallet_page", cached="false")
    manager = domain(root, "manager", "manager.elf", 94)
    mapping(manager, "image", 0x30000000, "rw")
    mapping(manager, "scratch", 0x31000000, "rw")
    mapping(manager, "chess_launch", 0x35000000, "rw", "chess_launch")
    mapping(manager, "next_launch", 0x35001000, "rw", "next_launch")
    ET.SubElement(ET.SubElement(manager, "cspace"), "cap_vspace", slot="1", pd="manager")
    for name, index, launch, namespace, addr, port, vault_port in (
        ("chess_slot", 1, "chess_launch", "notes_transfer", 0x32000000, 4, 3),
        ("next_slot", 2, "next_launch", "contacts_transfer", 0x33000000, 5, 4),
    ):
        pd = domain(manager, name, "bootstrap.elf", 96, id=str(index))
        mapping(pd, "image", 0x30000000, "rx")
        mapping(pd, "scratch", 0x31000000, "rw")
        mapping(pd, launch, 0x35000000, "r")
        mapping(pd, namespace, addr, "rw", "store_page", cached="false")
        channel(root, ("manager", index), (name, 0), bidirectional=keep_identity and index == 1)
        channel(root, (name, port), ("vault", vault_port), pp=True)
    channel(root, ("test_wallet", 0), ("manager", 0), bidirectional=True)
    channel(root, ("test_wallet", 8), ("vault", 8), pp=True)
    channel(root, ("test_wallet", 10), ("vault", 10), pp=True)
    # The vault emits a wakeup at unlock. The wallet uses a separate channel
    # for the manager's end-of-run readback request.
    channel(root, ("vault", 11), ("test_wallet", 11))
    return tree


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", required=True, type=Path, help="prepared UTM SDK and template.plist")
    parser.add_argument("--storage", required=True, type=Path, help="output of build_utm_vault_key_storage.py")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--control", choices=["keep-scratch", "keep-code-tail", "keep-identity"],
                        help="run a deliberately failing handoff")
    args = parser.parse_args()
    out, inputs, storage = (p.resolve() for p in (args.output, args.inputs, args.storage))
    if (any(p.suffix == ".utm" for p in (out, *out.parents)) or
            any(out.is_relative_to(p) or p.is_relative_to(out) for p in (inputs, storage))):
        raise ValueError("use a separate staging directory")
    out.mkdir(parents=True, exist_ok=True)
    sdk = inputs / "sdk"
    board = sdk / "board" / BOARD / CONFIG
    target = ROOT / "build/utm-wrapped-vault-cargo"
    env = dict(os.environ, SEL4_PREFIX=str(inputs / "sel4-install"), SEL4_INCLUDE_DIRS=str(board / "include"),
               CARGO_TARGET_DIR=str(target))
    run("cargo", "+nightly-2026-09-25", "build", "--locked", "--offline", "--release",
        "-Zjson-target-spec", "--config", ROOT / "rust/.cargo/config.toml", "--manifest-path", ROOT / "rust/Cargo.toml",
        "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json", "--features", "utm-wrapped-vault-experiment",
        "--bin", "rust_vault_disk", "--bin", "desktop_test_wallet", env=env)
    release = target / "aarch64-sel4-microkit/release"
    shutil.copyfile(release / "desktop_test_wallet.elf", out / "desktop_test_wallet.elf")
    objcopy = shutil.which("llvm-objcopy") or "/opt/homebrew/opt/llvm/bin/llvm-objcopy"
    run(objcopy, "--update-section", f".blk_client_config={storage / 'blk_client_vault.data'}",
        release / "rust_vault_disk.elf", out / "rust_vault_disk.elf")
    cc = ["clang", "--target=aarch64-none-elf", "-mcpu=cortex-a53", "-mgeneral-regs-only", "-O2",
          "-ffreestanding", "-fno-stack-protector", "-nostdlib", "-Wall", "-Wextra", "-Werror"]
    blobs = []
    for name, symbol in (("chess", "chess"), ("successor", "next")):
        run(*cc, "-c", SRC / f"{name}.c", "-o", out / f"{name}.o")
        run("ld.lld", "-T", SRC / "payload.ld", out / f"{name}.o", "-o", out / f"{name}.elf")
        run(objcopy, "-O", "binary", out / f"{name}.elf", out / f"{name}.bin")
        binary = (out / f"{name}.bin").read_bytes()
        if not 0 < len(binary) <= 4096:
            raise ValueError("app image exceeds code page")
        blobs.append(f".global {symbol}_start, {symbol}_end\n{symbol}_start:\n"
                     f'.incbin "{name}.bin"\n{symbol}_end:\n')
    (out / "packages.S").write_text(".section .rodata\n" + "".join(blobs))
    run(*cc, "-I", out, "-c", out / "packages.S", "-o", out / "packages.o")
    for name in ("manager", "bootstrap"):
        flags = []
        if args.control == "keep-scratch" and name == "manager":
            flags.append("-DCONTROL_KEEP_SCRATCH")
        if args.control == "keep-code-tail" and name == "manager":
            flags.append("-DCONTROL_KEEP_CODE_TAIL")
        if args.control == "keep-identity":
            flags.append("-DCONTROL_KEEP_IDENTITY")
        run(*cc, *flags, "-I", board / "include", "-c", SRC / f"{name}.c", "-o", out / f"{name}.o")
        extra = [out / "packages.o"] if name == "manager" else []
        run("ld.lld", "-L", board / "lib", out / f"{name}.o", *extra, "-lmicrokit", "-T", "microkit.ld",
            "-o", out / f"{name}.elf")
    tree = system(storage, args.control == "keep-identity")
    tree.write(out / "handoff.system", encoding="utf-8", xml_declaration=True)
    run(sdk / "bin/microkit", out / "handoff.system", "--search-path", out, "--search-path", storage,
        "--board", BOARD, "--config", CONFIG, "-o", out / "loader.img", "-r", out / "report.txt")
    markers = ["HANDOFF|PASS|wallet unlocked and readable", "HANDOFF|PASS|chess saved and reopened game",
               "HANDOFF|PASS|wallet file request denied to chess"]
    if args.control != "keep-identity":
        markers.append("HANDOFF|PASS|chess cannot read wallet page")
    if args.control == "keep-scratch":
        expected_failure = "HANDOFF|FAIL|old scratch bytes"
        markers.append(expected_failure)
        reject = "HANDOFF|PASS|reused scratch"
    elif args.control == "keep-code-tail":
        expected_failure = "HANDOFF|FAIL|old executable bytes"
        markers.append(expected_failure)
        reject = "HANDOFF|PASS|reused scratch"
    elif args.control == "keep-identity":
        expected_failure = "HANDOFF|FAIL|old chess capability reopened previous game"
        markers += ["HANDOFF|PASS|reused scratch and code tail clean",
                    expected_failure]
        reject = "HANDOFF|PASS|old storage capability absent"
    else:
        markers += ["HANDOFF|PASS|reused scratch and code tail clean",
                    "HANDOFF|PASS|old storage capability absent and new storage independent",
                    "HANDOFF|PASS|successor cannot read chess transfer page", "HANDOFF|PASS|wallet still readable",
                    "HANDOFF|PASS|complete"]
        reject = "HANDOFF|FAIL|"
    run(sys.executable, ROOT / "tests/utm_vault_key_probe.py", "--template", inputs / "template.plist",
        "--loader", out / "loader.img", "--blank-gpt", "--serial-output", out / "serial.log",
        *(arg for marker in markers for arg in ("--expect", marker)),
        "--reject", reject, "--reject", "MON|ERROR", "--reject", "Kernel panic")
    failures = [line.strip() for line in (out / "serial.log").read_text().splitlines()
                if "HANDOFF|FAIL|" in line]
    if failures != ([expected_failure] if args.control else []):
        raise RuntimeError(f"unexpected failure markers: {failures}")
    print("PASS: " + (f"detected deliberately broken {args.control}" if args.control else "isolated handoff"))


if __name__ == "__main__":
    main()
