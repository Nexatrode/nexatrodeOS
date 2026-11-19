#!/usr/bin/env python3
"""Build a private, no-test, no-device MPSS runtime-gate diagnostic image."""
from datetime import datetime, timezone
from pathlib import Path
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
PORT = ROOT / "platforms/sunfish"
FEATURE = "sunfish-native-startup-mpss-runtime-gate-diagnostic"
SYSTEM = PORT / "native-startup-mpss-auth.system"
PRIVATE_ROOT = ROOT / ".cache/sunfish-native-wifi/private"
IPA_DEFAULT = (
    ROOT / ".cache/sunfish-mobian-comparison/wifi-preparation/private/firmware"
    "/qcom/sm7150/google/sunfish/ipa_fws.mbn"
)
REFERENCE_DEFAULT = Path.home() / "Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
LIVE_DTB = ROOT / ".cache/pixel-sunfish/live-runtime.dtb"
KERNEL_DTB = (
    ROOT / ".cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
)


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def identity(path: Path) -> dict:
    return {"bytes": path.stat().st_size, "sha256": sha256(path)}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def private_inputs(inputs: Path) -> list[Path]:
    require(inputs.is_dir(), "private boot-input directory does not exist")
    require(inputs.stat().st_mode & 0o077 == 0,
            "private boot-input directory must not grant group/other access")
    manifest_path = inputs / "manifest.json"
    module_path = inputs / "private_boot_inputs.rs"
    require(manifest_path.is_file() and module_path.is_file(),
            "private boot inputs require manifest.json and private_boot_inputs.rs")
    manifest = json.loads(manifest_path.read_text())
    require(isinstance(manifest.get("files"), dict),
            "private input manifest has no files table")
    paths = [manifest_path, module_path]
    for name, record in manifest["files"].items():
        path = inputs / name
        require(path.parent == inputs and path.is_file(),
                "private input file is missing or outside the selected directory: " + name)
        require(path.stat().st_mode & 0o077 == 0,
                "private input file must not grant group/other access: " + name)
        require(sha256(path) == record.get("sha256"),
                "private input hash mismatch: " + name)
        paths.append(path)
    expected_module = manifest.get("module_sha256")
    require(expected_module and sha256(module_path) == expected_module,
            "private input module hash mismatch")
    return sorted(set(paths))


def allocate_output(requested: Path | None) -> Path:
    if requested:
        output = requested.expanduser().resolve()
        require(output.parent == PRIVATE_ROOT.resolve()
                or PRIVATE_ROOT.resolve() in output.parents,
                "output must remain under the private Sunfish cache")
    else:
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        output = PRIVATE_ROOT / f"native-mpss-runtime-gate-diagnostic-{stamp}"
        serial = 1
        while output.exists():
            output = PRIVATE_ROOT / f"native-mpss-runtime-gate-diagnostic-{stamp}-{serial:02d}"
            serial += 1
    require(not output.exists(), "refusing to reuse an existing diagnostic output directory")
    output.mkdir(parents=True, mode=0o700)
    output.chmod(0o700)
    return output


def run(command: list[str], *, cwd: Path, env: dict, log: Path,
        commands: list[list[str]]) -> None:
    commands.append(command)
    log.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with log.open("wb") as stream:
        process = subprocess.run(command, cwd=cwd, env=env, stdout=stream,
                                 stderr=subprocess.STDOUT, check=False)
    if process.returncode:
        tail = log.read_text(errors="replace").splitlines()[-35:]
        raise RuntimeError(
            f"command failed ({process.returncode}): {command!r}\n" + "\n".join(tail)
        )


def source_inventory(inputs: list[Path], output: Path, reference: Path,
                     firmware: list[Path]) -> dict:
    paths = set((ROOT / "rust/src").rglob("*.rs"))
    paths.update((ROOT / "rust/targets").glob("*.json"))
    paths.update(ROOT / "rust" / name for name in ("Cargo.toml", "Cargo.lock"))
    paths.update(
        path for path in PORT.rglob("*")
        if path.is_file() and path.suffix in {".py", ".sh", ".system", ".json", ".h", ".inc"}
    )
    paths.update(inputs)
    paths.add(reference)
    paths.update(firmware)
    paths.add(SYSTEM)
    paths = sorted(path.resolve() for path in paths if path.is_file())
    return {str(path): identity(path) for path in paths}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, required=True,
                        help="prepared private boot-input directory")
    parser.add_argument("--output-dir", type=Path,
                        help="fresh directory under .cache/sunfish-native-wifi/private")
    parser.add_argument("--ipa-firmware", type=Path, default=IPA_DEFAULT)
    parser.add_argument("--reference-boot", type=Path, default=REFERENCE_DEFAULT)
    args = parser.parse_args()

    os.umask(0o077)
    inputs = args.inputs.expanduser().resolve()
    input_paths = private_inputs(inputs)
    ipa_source = args.ipa_firmware.expanduser().resolve()
    reference = args.reference_boot.expanduser().resolve()
    require(ipa_source.is_file() and reference.is_file(),
            "required IPA firmware or reference boot image is missing")
    require(ipa_source.stat().st_size == 29_120
            and sha256(ipa_source) == "7d61b64288248719d14a4b316e9cb83917c11a2c9753dff5475a7a2db95394dd",
            "wrong private Sunfish IPA firmware identity")
    require(SYSTEM.is_file() and LIVE_DTB.is_file() and KERNEL_DTB.is_file(),
            "auth system or required device-tree inputs are missing")

    output = allocate_output(args.output_dir)
    commands: list[list[str]] = []
    command_env = os.environ.copy()
    command_env["PATH"] = "/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:" + command_env.get("PATH", "")
    command_env["SEL4_INCLUDE_DIRS"] = str(
        output / "sdk/board/sunfish/debug/include"
    )
    command_env["CARGO_TARGET_DIR"] = str(output / "cargo-target")
    command_env["SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH"] = str(inputs / "private_boot_inputs.rs")

    ipa_copy = output / "private-ipa_fws.mbn"
    shutil.copyfile(ipa_source, ipa_copy)
    ipa_copy.chmod(0o600)
    command_env["SUNFISH_IPA_FIRMWARE_PATH"] = str(ipa_copy)

    audit = PORT / "audit-native-mpss-runtime-gate-diagnostic.py"
    run([sys.executable, str(audit), "--output", str(output / "source-audit.json")],
        cwd=ROOT, env=command_env, log=output / "source-audit.log", commands=commands)

    run([sys.executable, str(PORT / "prepare-image-loader-sdk.py"),
         "--out-dir", str(output), "--checkpoints", "--native-source-report-guard"],
        cwd=ROOT, env=command_env, log=output / "loader-sdk-build.log", commands=commands)
    sdk = output / "sdk"
    require((sdk / "bin/microkit").is_file(), "private loader SDK did not produce Microkit")

    inventory = source_inventory(
        input_paths, output, reference, [ipa_source, ipa_copy]
    )
    (output / "precompile-inputs.json").write_text(json.dumps({
        "feature": FEATURE,
        "system": str(SYSTEM.resolve()),
        "files": inventory,
    }, indent=2) + "\n")

    cargo = [
        "cargo", "+nightly-2026-09-25", "build", "--locked", "--offline", "--release",
        "--manifest-path", str(ROOT / "rust/Cargo.toml"),
        "--features", FEATURE,
        "--bin", "rust_sunfish_native_startup_preflight",
        "--target", str(ROOT / "rust/targets/aarch64-sel4-microkit-pixel4k.json"),
        "-Z", "build-std=core,alloc", "-Z", "json-target-spec",
    ]
    run(cargo, cwd=ROOT, env=command_env, log=output / "cargo-build.log", commands=commands)
    elf = Path(command_env["CARGO_TARGET_DIR"]) / (
        "aarch64-sel4-microkit-pixel4k/release/rust_sunfish_native_startup_preflight.elf"
    )
    require(elf.is_file(), "Cargo did not produce the diagnostic AArch64 ELF")
    shutil.copyfile(elf, output / "native_startup_preflight.elf")

    run([str(sdk / "bin/microkit"), str(SYSTEM), "--search-path", str(output),
         "--board", "sunfish", "--config", "debug", "-o", str(output / "loader.img"),
         "-r", str(output / "report.txt"), "--capdl-json", str(output / "capdl.json")],
        cwd=ROOT, env=command_env, log=output / "microkit-build.log", commands=commands)
    run([sys.executable, str(PORT / "validate-native-mpss-auth.py"),
         "--system", str(SYSTEM), "--capdl", str(output / "capdl.json"),
         "--report", str(output / "report.txt"), "--dt", str(LIVE_DTB),
         "--dt", str(KERNEL_DTB), "--output", str(output / "capability-audit.json")],
        cwd=ROOT, env=command_env, log=output / "capability-audit.log", commands=commands)

    image = output / "native-startup-preflight.Image"
    image_lz4 = output / "native-startup-preflight.Image.lz4"
    run([sys.executable, str(PORT / "wrap-microkit-image.py"),
         str(output / "loader.img"), str(image)],
        cwd=ROOT, env=command_env, log=output / "wrap-image.log", commands=commands)
    run(["lz4", "-z", "-f", "-q", str(image), str(image_lz4)],
        cwd=ROOT, env=command_env, log=output / "lz4.log", commands=commands)
    run([sys.executable, str(PORT / "repack-boot-v2.py"), str(reference),
         str(image_lz4), str(output / "fastboot.img")],
        cwd=ROOT, env=command_env, log=output / "repack-boot.log", commands=commands)
    run([sys.executable, str(PORT / "validate-native-mpss-auth-boot.py"),
         "--output-dir", str(output), "--reference", str(reference)],
        cwd=ROOT, env=command_env, log=output / "boot-layout-audit.log", commands=commands)
    run([sys.executable, str(audit), "--output", str(output / "source-audit-final.json")],
        cwd=ROOT, env=command_env, log=output / "source-audit-final.log", commands=commands)

    artifacts = [
        output / "fastboot.img", output / "native_startup_preflight.elf",
        output / "loader.img", output / "native-startup-preflight.Image.lz4",
        output / "capdl.json", output / "report.txt", output / "capability-audit.json",
        output / "boot-layout-audit.json", output / "source-audit-final.json",
    ]
    require(all(path.is_file() for path in artifacts), "diagnostic artifact set is incomplete")
    manifest = {
        "schema": 1,
        "purpose": "temporary Sunfish auth/runtime gate diagnostic",
        "feature": FEATURE,
        "system_profile": str(SYSTEM.relative_to(ROOT)),
        "source_guard_mode": "labeled_failures_continue_to_runtime_gate_on_pass",
        "stop_boundary": "before take_runtime_boot_parts and PAS4",
        "mpss_firmware_embedded": False,
        "device_boot_performed": False,
        "pas4_attempted": False,
        "owner_handoff_attempted": False,
        "reset_released": False,
        "glink_qrtr_started": False,
        "wlan_started": False,
        "flash_performed": False,
        "retry_performed": False,
        "test_suites_invoked": False,
        "commands": commands,
        "input_sha256": inventory,
        "artifact_sha256": {path.name: identity(path) for path in artifacts},
    }
    manifest_path = output / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    manifest_path.chmod(0o600)
    (output / "fastboot.img.sha256").write_text(
        hashlib.sha256((output / "fastboot.img").read_bytes()).hexdigest()
        + "  fastboot.img\n"
    )
    (output / "fastboot.img.sha256").chmod(0o600)
    print("Built offline diagnostic image; no test suites or device commands were run.")
    print("Output directory:", output)
    print("Fastboot image SHA-256:", sha256(output / "fastboot.img"))


if __name__ == "__main__":
    main()
