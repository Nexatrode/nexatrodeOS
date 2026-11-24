#!/usr/bin/env python3
"""Build isolated UTM storage fixture; never provision a real vault key."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
BOARD = "qemu_virt_aarch64_el1"
CONFIG = "smp-debug"


def run(*args, **kwargs):
    subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--inputs", required=True, type=Path,
                        help="prepared UTM SDK and seL4 installation directory")
    parser.add_argument("--sddf", required=True, type=Path)
    parser.add_argument("--sddf-python", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    inputs, sddf, output = (p.resolve() for p in (args.inputs, args.sddf, args.output))
    output.mkdir(parents=True, exist_ok=True)
    sdk, target = inputs / "sdk", ROOT / "build/utm-accelerated-ui-cargo"
    env = os.environ.copy()
    env.update(SEL4_PREFIX=str(inputs / "sel4-install"),
               SEL4_INCLUDE_DIRS=str(sdk / "board" / BOARD / CONFIG / "include"),
               CARGO_TARGET_DIR=str(target),
               PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/sbin:" + env.get("PATH", ""))
    run("cargo", "+nightly-2026-09-25", "build", "--locked", "--offline", "--release",
        "-Zjson-target-spec", "--config", ROOT / "rust/.cargo/config.toml", "--manifest-path",
        ROOT / "rust/Cargo.toml", "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json",
        "--features", "vault-key-prototype,storage-test", "--bin", "rust_vault_key_storage_probe",
        "--bin", "rust_vault_disk", "--bin", "rust_entropy_rng", cwd=ROOT, env=env)
    release = target / "aarch64-sel4-microkit/release"
    for source, dest in (("rust_vault_disk.elf", "rust_vault_disk.elf"),
                         ("rust_entropy_rng.elf", "rust_entropy_rng.elf"),
                         ("rust_vault_key_storage_probe.elf", "rust_vault_test_client.elf")):
        shutil.copyfile(release / source, output / dest)

    # sDDF's build expects its standard board name; the accelerated SDK uses
    # a distinct name for the same device layout and its four-core kernel.
    alias = output / "sdk"
    (alias / "board").mkdir(parents=True, exist_ok=True)
    for link, destination in ((alias / "bin", sdk / "bin"),
                              (alias / "board/qemu_virt_aarch64", sdk / "board" / BOARD)):
        if not link.exists(): link.symlink_to(destination, target_is_directory=True)
        if link.resolve() != destination.resolve(): raise RuntimeError("unexpected SDK alias")
    shutil.copyfile(sddf / "examples/blk/blk.mk", output / "Makefile")
    run("make", "-C", output, f"MICROKIT_SDK={alias}", "MICROKIT_BOARD=qemu_virt_aarch64",
        f"MICROKIT_CONFIG={CONFIG}", f"SDDF={sddf}", f"PYTHON={args.sddf_python.absolute()}",
        f"METAPROGRAM={ROOT / 'tests/storage_meta.py'}", "SYSTEM_FILE=storage.system",
        "storage.system", cwd=ROOT, env=env)
    # The general storage test maps two pages that this focused probe does not
    # access. Keep the mappings, but remove symbols that the probe doesn't have.
    system = output / "storage.system"
    tree = ET.parse(system)
    tester = tree.getroot().find("protection_domain[@name='storage_test']")
    if tester is None: raise RuntimeError("missing storage test PD")
    for mapping in tester.findall("map"):
        if mapping.get("setvar_vaddr") in ("messages_transfer", "wallet_identity_transfer"):
            mapping.attrib.pop("setvar_vaddr")
    tree.write(system, encoding="utf-8", xml_declaration=True)
    run("llvm-objcopy", "--update-section", f".blk_client_config={output / 'blk_client_vault.data'}",
        output / "rust_vault_disk.elf", env=env)
    print("Built disposable-only storage fixture in", output, flush=True)


if __name__ == "__main__":
    main()
