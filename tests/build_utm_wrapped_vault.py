#!/usr/bin/env python3
"""Build a UTM image for blank-disk wrapped-vault testing only.

The experimental vault is built without an installation key. This script
extracts only the block-client configuration from the prepared vault ELF.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
BOARD, CONFIG = "qemu_virt_aarch64_el1", "smp-debug"


def run(*args, env=None):
    subprocess.run([str(x) for x in args], cwd=ROOT, env=env, check=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--inputs", required=True, type=Path)
    parser.add_argument("--sddf", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--gated-agent", required=True, type=Path, help="test-only SimpleX agent built with the startup gate")
    args = parser.parse_args()
    inputs, output = args.inputs.resolve(), args.output.resolve()
    if output.is_relative_to(inputs) or any(p.suffix == ".utm" for p in (output, *output.parents)):
        raise ValueError("output must be a staging directory outside VM bundles and inputs")
    output.mkdir(parents=True, exist_ok=True)
    sdk, pds = inputs / "sdk", inputs / "pds"
    env = os.environ.copy()
    env.update(SEL4_PREFIX=str(inputs / "sel4-install"), SEL4_INCLUDE_DIRS=str(sdk / "board" / BOARD / CONFIG / "include"))
    target = ROOT / "build/utm-wrapped-vault-cargo"
    env["CARGO_TARGET_DIR"] = str(target)
    flags = ["--locked", "--offline", "--release", "-Zjson-target-spec", "--config", ROOT / "rust/.cargo/config.toml"]
    run("cargo", "+nightly-2026-09-25", "build", *flags, "--manifest-path", ROOT / "rust/Cargo.toml",
        "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json", "--features",
        "utm-wrapped-vault-experiment,simplex-agent-vault,utm-photo-reader", "--bin", "rust_vault_disk", env=env)
    target_ui = ROOT / "build/utm-wrapped-ui-cargo"
    env["CARGO_TARGET_DIR"] = str(target_ui)
    run("cargo", "+nightly-2026-09-25", "build", *flags, "--manifest-path", ROOT / "experiments/slint-notes/Cargo.toml",
        "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json", "--features",
        "utm-pd,full-suite-ui,simplex-agent-ui,utm-photo-worker,utm-transcript-worker,utm-wrapped-vault-experiment",
        "--bin", "slint_notes_utm", "--bin", "slint_photo_worker", "--bin", "slint_photo_reader",
        "--bin", "slint_transcript_worker", env=env)
    run("cargo", "+nightly-2026-09-25", "build", *flags, "--manifest-path", ROOT / "rust/Cargo.toml",
        "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json", "--features",
        "utm-slint-input,utm-transcript-worker,utm-device-lock", "--bin", "rust_input_service",
        "--bin", "rust_messages_agent_app", "--bin", "rust_entropy_rng", env=env)
    os.environ["NEXATRODE_SDDF"] = str(args.sddf.resolve())
    spec = importlib.util.spec_from_file_location("accelerated", ROOT / "experiments/hvf-smp/accelerated.py")
    accelerated = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(accelerated)
    objcopy = Path("/opt/homebrew/opt/llvm/bin/llvm-objcopy")
    os.umask(0o077)
    with tempfile.TemporaryDirectory(dir=output, prefix="stage-") as tmp:
        stage = Path(tmp)
        ui, vault, agent = stage / "ui", stage / "vault", stage / "agent"
        ui.mkdir(); vault.mkdir(); agent.mkdir()
        shutil.copyfile(args.gated_agent, agent / "agentstore_probe.elf")
        for name in ("slint_notes_utm.elf", "slint_photo_worker.elf", "slint_photo_reader.elf", "slint_transcript_worker.elf"):
            shutil.copyfile(target_ui / "aarch64-sel4-microkit/release" / name, ui / name)
        for source_name, dest_name in (("rust_input_service.elf", "input_service.elf"),
                                       ("rust_messages_agent_app.elf", "rust_messages_agent_app.elf"),
                                       ("rust_entropy_rng.elf", "rust_entropy_rng.elf")):
            shutil.copyfile(target_ui / "aarch64-sel4-microkit/release" / source_name, ui / dest_name)
        source = target / "aarch64-sel4-microkit/release/rust_vault_disk.elf"
        config = stage / "block-config"
        run(objcopy, "--dump-section", f".blk_client_config={config}", pds / "rust_vault_disk.elf")
        run(objcopy, "--update-section", f".blk_client_config={config}", source, vault / "rust_vault_disk.elf")
        sections = subprocess.run([objcopy, "--dump-section", f".utm_vault_key={stage / 'unexpected-key'}", vault / "rust_vault_disk.elf"], capture_output=True)
        if sections.returncode == 0 and (stage / "unexpected-key").exists():
            raise RuntimeError("experimental vault unexpectedly has an installation key section")
        gpu = accelerated.build_gpu_from_source(inputs, stage)
        system = ET.parse(ROOT / "experiments/hvf-smp/accelerated.system")
        for pd, channel in (("vault", "11"), ("simplex_agent", "6")):
            if any(end.get("pd") == pd and end.get("id") == channel for end in system.findall("channel/end")):
                raise RuntimeError("experimental startup notification channel is already used")
        link = ET.SubElement(system.getroot(), "channel")
        ET.SubElement(link, "end", {"pd": "vault", "id": "11", "notify": "true"})
        ET.SubElement(link, "end", {"pd": "simplex_agent", "id": "6", "notify": "false"})
        system_path = stage / "experimental.system"
        system.write(system_path, encoding="utf-8", xml_declaration=True)
        run(sdk / "bin/microkit", system_path, "--board", BOARD, "--config", CONFIG,
            "--search-path", gpu, "--search-path", ui, "--search-path", vault, "--search-path", agent, "--search-path", pds,
            "-o", stage / "loader.img", "-r", stage / "report.txt")
        shutil.copyfile(stage / "loader.img", output / "loader.img")
        shutil.copyfile(stage / "report.txt", output / "report.txt")
    print("Built experimental blank-disk loader in", output)


if __name__ == "__main__":
    main()
