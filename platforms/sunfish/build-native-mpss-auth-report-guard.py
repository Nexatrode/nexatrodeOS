#!/usr/bin/env python3
"""Build and freeze-ready audit the one-shot PAS4/Q6-ready image, offline."""
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
FEATURE = "sunfish-native-startup-mpss-auth"
SYSTEM = PORT / "native-startup-mpss-auth.system"
PRIVATE_ROOT = ROOT / ".cache/sunfish-native-wifi/private"
INPUTS_DEFAULT = PRIVATE_ROOT / "prepared-boot-inputs-20261025"
MODEM_DEFAULT = ROOT / ".cache/sunfish-mobian-comparison/wifi-preparation/private/firmware/qcom/sm7150/google/sunfish/modem.mbn"
IPA_DEFAULT = ROOT / ".cache/sunfish-mobian-comparison/wifi-preparation/private/firmware/qcom/sm7150/google/sunfish/ipa_fws.mbn"
REFERENCE_DEFAULT = Path.home() / "Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
LIVE_DTB = ROOT / ".cache/pixel-sunfish/live-runtime.dtb"
KERNEL_DTB = ROOT / ".cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def identity(path: Path) -> dict:
    return {"bytes": path.stat().st_size, "sha256": sha256(path)}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def run(command: list[str], *, cwd: Path, env: dict, output: Path,
        commands: list[list[str]]) -> None:
    commands.append(command)
    with output.open("wb") as log:
        result = subprocess.run(command, cwd=cwd, env=env, stdout=log,
                                stderr=subprocess.STDOUT, check=False)
    if result.returncode:
        tail = output.read_text(errors="replace").splitlines()[-40:]
        raise RuntimeError(f"command failed ({result.returncode}): {command!r}\n" + "\n".join(tail))


def private_input_paths(inputs: Path) -> list[Path]:
    require(inputs.is_dir() and not (inputs.stat().st_mode & 0o077),
            "private boot-input directory must exist with mode 0700")
    manifest_path = inputs / "manifest.json"
    module_path = inputs / "private_boot_inputs.rs"
    require(manifest_path.is_file() and module_path.is_file(),
            "private boot inputs are incomplete")
    manifest = json.loads(manifest_path.read_text())
    require(isinstance(manifest.get("files"), dict), "private input manifest has no file table")
    paths = [manifest_path, module_path]
    for name, record in manifest["files"].items():
        path = inputs / name
        require(path.parent == inputs and path.is_file(), "missing private input: " + name)
        require(not (path.stat().st_mode & 0o077), "private input permissions are too broad: " + name)
        require(sha256(path) == record.get("sha256"), "private input hash mismatch: " + name)
        paths.append(path)
    require(sha256(module_path) == manifest.get("module_sha256"),
            "private Rust input module hash mismatch")
    return sorted(set(paths))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, default=INPUTS_DEFAULT)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--modem-firmware", type=Path, default=MODEM_DEFAULT)
    parser.add_argument("--ipa-firmware", type=Path, default=IPA_DEFAULT)
    parser.add_argument("--reference-boot", type=Path, default=REFERENCE_DEFAULT)
    args = parser.parse_args()

    os.umask(0o077)
    inputs = args.inputs.expanduser().resolve()
    input_paths = private_input_paths(inputs)
    modem = args.modem_firmware.expanduser().resolve()
    ipa = args.ipa_firmware.expanduser().resolve()
    reference = args.reference_boot.expanduser().resolve()
    require(modem.is_file() and identity(modem) == {
        "bytes": 68_460_672,
        "sha256": "8489e6828dc12ed98e26dec15adb7102a31aafaf7314e249854c737ddba4cdc0",
    }, "wrong pinned private Sunfish MPSS firmware")
    require(ipa.is_file() and identity(ipa) == {
        "bytes": 29_120,
        "sha256": "7d61b64288248719d14a4b316e9cb83917c11a2c9753dff5475a7a2db95394dd",
    }, "wrong pinned private Sunfish IPA firmware")
    require(reference.is_file() and SYSTEM.is_file() and LIVE_DTB.is_file()
            and KERNEL_DTB.is_file(), "required system, boot or DT inputs are missing")

    if args.output_dir:
        output = args.output_dir.expanduser().resolve()
        require(PRIVATE_ROOT.resolve() in output.parents,
                "output must be under the private Sunfish cache")
    else:
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        output = PRIVATE_ROOT / f"native-mpss-auth-report-guard-{stamp}"
        suffix = 1
        while output.exists():
            output = PRIVATE_ROOT / f"native-mpss-auth-report-guard-{stamp}-{suffix:02d}"
            suffix += 1
    require(not output.exists(), "refusing to reuse an existing build directory")
    output.mkdir(parents=True, mode=0o700)
    output.chmod(0o700)

    commands: list[list[str]] = []
    env = os.environ.copy()
    env["PATH"] = "/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:" + env.get("PATH", "")
    env["SEL4_INCLUDE_DIRS"] = str(output / "sdk/board/sunfish/debug/include")
    # This target path is also the one recorded by freeze-native-mpss-auth.py.
    env["CARGO_TARGET_DIR"] = str(ROOT / ".cache/sunfish-native-wifi/private/preflight-target")
    env["SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH"] = str(inputs / "private_boot_inputs.rs")
    env["SUNFISH_MPSS_FIRMWARE_PATH"] = str(output / "private-modem.mbn")
    env["SUNFISH_IPA_FIRMWARE_PATH"] = str(output / "private-ipa_fws.mbn")
    shutil.copyfile(modem, output / "private-modem.mbn")
    shutil.copyfile(ipa, output / "private-ipa_fws.mbn")
    (output / "private-modem.mbn").chmod(0o600)
    (output / "private-ipa_fws.mbn").chmod(0o600)

    run([sys.executable, str(PORT / "prepare-image-loader-sdk.py"),
         "--out-dir", str(output), "--checkpoints", "--native-source-report-guard"],
        cwd=ROOT, env=env, output=output / "loader-sdk-build.log", commands=commands)
    sdk = output / "sdk"
    require((sdk / "bin/microkit").is_file(), "private Microkit SDK was not built")

    before = [sys.executable, str(PORT / "snapshot-native-mpss-auth-inputs.py"),
              "--output-dir", str(output), "--inputs", str(inputs)]
    run(before, cwd=ROOT, env=env, output=output / "snapshot.log", commands=commands)
    audit = PORT / "audit-native-mpss-auth-source.py"
    run([sys.executable, str(audit), "--output", str(output / "mpss-auth-source-audit.json")],
        cwd=ROOT, env=env, output=output / "mpss-auth-source-audit.log", commands=commands)

    cargo = ["cargo", "+nightly-2026-09-25", "build", "--locked", "--offline", "--release",
             "--manifest-path", str(ROOT / "rust/Cargo.toml"), "--features", FEATURE,
             "--bin", "rust_sunfish_native_startup_preflight", "--target",
             str(ROOT / "rust/targets/aarch64-sel4-microkit-pixel4k.json"),
             "-Z", "build-std=core,alloc", "-Z", "json-target-spec"]
    run(cargo, cwd=ROOT, env=env, output=output / "cargo-build.log", commands=commands)
    elf = Path(env["CARGO_TARGET_DIR"]) / (
        "aarch64-sel4-microkit-pixel4k/release/rust_sunfish_native_startup_preflight.elf"
    )
    require(elf.is_file(), "Cargo did not produce the PAS4 AArch64 ELF")
    shutil.copyfile(elf, output / "native_startup_preflight.elf")

    run([str(sdk / "bin/microkit"), str(SYSTEM), "--search-path", str(output),
         "--board", "sunfish", "--config", "debug", "-o", str(output / "loader.img"),
         "-r", str(output / "report.txt"), "--capdl-json", str(output / "capdl.json")],
        cwd=ROOT, env=env, output=output / "microkit-build.log", commands=commands)
    run([sys.executable, str(PORT / "validate-native-mpss-auth.py"), "--system", str(SYSTEM),
         "--capdl", str(output / "capdl.json"), "--report", str(output / "report.txt"),
         "--dt", str(LIVE_DTB), "--dt", str(KERNEL_DTB), "--output",
         str(output / "capability-audit.json")],
        cwd=ROOT, env=env, output=output / "capability-audit.log", commands=commands)

    image = output / "native-startup-preflight.Image"
    compressed = output / "native-startup-preflight.Image.lz4"
    run([sys.executable, str(PORT / "wrap-microkit-image.py"), str(output / "loader.img"), str(image)],
        cwd=ROOT, env=env, output=output / "wrap-image.log", commands=commands)
    run(["lz4", "-z", "-f", "-q", str(image), str(compressed)], cwd=ROOT, env=env,
        output=output / "lz4.log", commands=commands)
    run([sys.executable, str(PORT / "repack-boot-v2.py"), str(reference), str(compressed),
         str(output / "fastboot.img")], cwd=ROOT, env=env,
        output=output / "repack-boot.log", commands=commands)
    run([sys.executable, str(PORT / "validate-native-mpss-auth-boot.py"), "--output-dir",
         str(output), "--reference", str(reference)], cwd=ROOT, env=env,
        output=output / "boot-layout-audit.log", commands=commands)

    artifacts = ["fastboot.img", "native_startup_preflight.elf", "loader.img",
                 "native-startup-preflight.Image.lz4", "capdl.json", "report.txt",
                 "capability-audit.json", "boot-layout-audit.json", "mpss-auth-source-audit.json"]
    require(all((output / name).is_file() for name in artifacts), "artifact set is incomplete")
    summary = {
        "schema": 1,
        "purpose": "one-shot native Sunfish MPSS PAS4 and Q6-ready check",
        "feature": FEATURE,
        "scope": "Existing IPA/RMTFS/power prerequisites, PAS4 authentication, then delivered Q6-ready observation. Stops before GLINK/QRTR/WLAN; no flash.",
        "source_guard_mode": "labeled failures; pass continues to relocation and PAS4",
        "stop_boundary": "after PAS4/Q6-ready receipt; before GLINK/QRTR/WLAN",
        "device_boot_performed": False,
        "pas4_attempted": False,
        "reset_released": False,
        "glink_qrtr_started": False,
        "wlan_started": False,
        "flash_performed": False,
        "retry_performed": False,
        "test_suites_invoked": False,
        "commands": commands,
        "private_inputs": {str(path): identity(path) for path in input_paths},
        "firmware": {"modem": identity(output / "private-modem.mbn"),
                     "ipa": identity(output / "private-ipa_fws.mbn")},
        "artifacts": {name: identity(output / name) for name in artifacts},
        "frozen": True,
        "attempt_consumed": False,
        "fastboot_attempted": False,
        "next_action": "One cold temporary fastboot boot only after user readiness; capture PAS4/auth and Q6-ready results, then return to LineageOS.",
    }
    (output / "build-manifest.json").write_text(json.dumps(summary, indent=2) + "\n")
    (output / "build-manifest.json").chmod(0o600)
    image_hash = sha256(output / "fastboot.img")
    (output / "fastboot.img.sha256").write_text(f"{image_hash}  fastboot.img\n")
    (output / "fastboot.img.sha256").chmod(0o600)
    print("Built and audited offline; no test suites or device commands were run.")
    print("Output directory:", output)
    print("Fastboot image SHA-256:", image_hash)


if __name__ == "__main__":
    main()
