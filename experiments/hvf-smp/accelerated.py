"""Build the regular four-core UTM image using the measured NexaWal release."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
INPUTS = ROOT / ".cache/utm-accelerated"
PROFILE = json.loads((HERE / "accelerated.lock.json").read_text())
BOARD = "qemu_virt_aarch64_el1"
CONFIG = "smp-debug"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def vault_source_required(pds):
    vault = pds / "rust_vault_disk.elf"
    return not (vault.is_file() and b"photo_read_transfer" in vault.read_bytes()
                and b"device_lock_transfer" in vault.read_bytes())


def ui_source_required(pds):
    ui = pds / "slint_notes_utm.elf"
    worker = pds / "slint_photo_worker.elf"
    reader = pds / "slint_photo_reader.elf"
    transcript = pds / "slint_transcript_worker.elf"
    messages = pds / "rust_messages_agent_app.elf"
    entropy = pds / "rust_entropy_rng.elf"
    if not (ui.is_file() and worker.is_file() and reader.is_file() and transcript.is_file() and messages.is_file() and entropy.is_file()):
        return True
    binary = ui.read_bytes()
    return (b"SLINT|FRAME_COPY_RAM_V1" not in binary or b"slint_clock_view" not in binary or b"device_lock_transfer" not in binary or b"ENTROPY|DEVICE_LOCK_V1" not in entropy.read_bytes()
            or b"photo_mailbox_region" not in binary or b"photo_read_mailbox" not in binary
            or b"transcript_mailbox" not in binary or b"messages_worker_transfer" not in messages.read_bytes()
            or b"LIVE_RECEIVE_WORKER_ABI_V1" not in messages.read_bytes()
            or b"NAVIGATION_WORKER_ABI_V1" not in messages.read_bytes()
            or b"SNAPSHOT_WORKER_ABI_V1" not in messages.read_bytes()
            or b"live_receive_mailbox" not in binary or b"live_receive_mailbox" not in transcript.read_bytes()
            or b"navigation_mailbox" not in binary or b"navigation_mailbox" not in transcript.read_bytes()
            or b"snapshot_mailbox" not in binary or b"snapshot_mailbox" not in transcript.read_bytes())


def build_ui_from_source(inputs, stage):
    target = Path(os.environ.get("NEXATRODE_UI_TARGET_DIR", str(ROOT / "build/utm-accelerated-ui-cargo"))).resolve()
    env = dict(os.environ, SEL4_PREFIX=str(inputs / "sel4-install"), CARGO_TARGET_DIR=str(target),
               SEL4_INCLUDE_DIRS=str(inputs / "sdk/board" / BOARD / CONFIG / "include"))
    # Cargo must rebuild the UI if the Mac switches timezone, even when the
    # newly selected zoneinfo file happens to have the same modification time.
    env["NEXATRODE_HOST_TIMEZONE_SHA256"] = digest(Path("/etc/localtime"))
    env.setdefault("CARGO_BUILD_JOBS", "2")
    features = "utm-pd,full-suite-ui,simplex-agent-ui,utm-photo-worker,utm-transcript-worker,utm-device-lock"
    if env.get("NEXATRODE_UTM_PERF_TRACE") == "1":
        features += ",utm-perf-trace"
    subprocess.run([
        "cargo", "+nightly-2026-09-25", "build", "--locked", "--offline", "--release",
        "-Zjson-target-spec", "--config", ROOT / "rust/.cargo/config.toml",
        "--manifest-path", ROOT / "experiments/slint-notes/Cargo.toml",
        "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json",
        "--features", features,
        "--bin", "slint_notes_utm", "--bin", "slint_photo_worker", "--bin", "slint_photo_reader", "--bin", "slint_transcript_worker",
    ], cwd=ROOT, env=env, check=True)
    subprocess.run([
        "cargo", "+nightly-2026-09-25", "build", "--locked", "--offline", "--release",
        "-Zjson-target-spec", "--config", ROOT / "rust/.cargo/config.toml",
        "--manifest-path", ROOT / "rust/Cargo.toml",
        "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json",
        "--features", "utm-slint-input,utm-transcript-worker,utm-device-lock", "--bin", "rust_input_service",
        "--bin", "rust_messages_agent_app", "--bin", "rust_entropy_rng",
    ], cwd=ROOT, env=env, check=True)
    output = stage / "ui"
    output.mkdir()
    release = target / "aarch64-sel4-microkit/release"
    for name in ("slint_notes_utm.elf", "slint_photo_worker.elf", "slint_photo_reader.elf", "slint_transcript_worker.elf"):
        shutil.copyfile(release / name, output / name)
    shutil.copyfile(release / "rust_input_service.elf", output / "input_service.elf")
    shutil.copyfile(release / "rust_messages_agent_app.elf", output / "rust_messages_agent_app.elf")
    shutil.copyfile(release / "rust_entropy_rng.elf", output / "rust_entropy_rng.elf")
    return output


def build_gpu_from_source(inputs, stage):
    sddf = Path(os.environ.get("NEXATRODE_SDDF", str(ROOT / ".cache/sddf-personal-os"))).resolve()
    if not (sddf / "examples/gpu/gpu.mk").is_file():
        raise FileNotFoundError("sDDF source missing; set NEXATRODE_SDDF to the prepared sddf-personal-os checkout")
    build_dir = stage / "gpu"
    # sDDF recognizes the standard QEMU board name; use the accelerated
    # kernel's matching headers and libraries through a private SDK alias.
    sdk_alias = stage / "gpu-sdk"
    (sdk_alias / "board").mkdir(parents=True)
    (sdk_alias / "bin").symlink_to(inputs / "sdk/bin", target_is_directory=True)
    (sdk_alias / "board/qemu_virt_aarch64").symlink_to(inputs / "sdk/board" / BOARD, target_is_directory=True)
    cmd = ["python3", ROOT / "experiments/ghc-microkit/build_slint_gpu_client.py",
           "--sdk", sdk_alias, "--sddf", sddf, "--build", build_dir,
           "--board", "qemu_virt_aarch64", "--config", CONFIG]
    if os.environ.get("NEXATRODE_UTM_PERF_TRACE") == "1":
        cmd.append("--perf-trace")
    subprocess.run(cmd, cwd=ROOT, check=True)
    shutil.copyfile(build_dir / "client.elf", build_dir / "gpu_client.elf")
    return build_dir


def build_vault_from_source(inputs, stage):
    target = Path(os.environ.get("NEXATRODE_UI_TARGET_DIR", str(ROOT / "build/utm-accelerated-ui-cargo"))).resolve()
    env = dict(os.environ, SEL4_PREFIX=str(inputs / "sel4-install"), CARGO_TARGET_DIR=str(target),
               SEL4_INCLUDE_DIRS=str(inputs / "sdk/board" / BOARD / CONFIG / "include"))
    env.setdefault("CARGO_BUILD_JOBS", "2")
    subprocess.run([
        "cargo", "+nightly-2026-09-25", "build", "--locked", "--offline", "--release",
        "-Zjson-target-spec", "--config", ROOT / "rust/.cargo/config.toml",
        "--manifest-path", ROOT / "rust/Cargo.toml",
        "--target", ROOT / "rust/targets/aarch64-sel4-microkit.json",
        "--features", "utm-installation-vault-key,simplex-agent-vault,utm-photo-reader,utm-device-lock",
        "--bin", "rust_vault_disk",
    ], cwd=ROOT, env=env, check=True)
    objcopy = os.environ.get("NEXATRODE_LLVM_OBJCOPY") or shutil.which("llvm-objcopy")
    if not objcopy and Path("/opt/homebrew/opt/llvm/bin/llvm-objcopy").is_file():
        objcopy = "/opt/homebrew/opt/llvm/bin/llvm-objcopy"
    if not objcopy:
        raise FileNotFoundError("llvm-objcopy is required to preserve the UTM vault configuration")
    old = inputs / "pds/rust_vault_disk.elf"
    compiled = target / "aarch64-sel4-microkit/release/rust_vault_disk.elf"
    output = stage / "vault"
    output.mkdir(mode=0o700)
    with tempfile.TemporaryDirectory(dir=stage) as tmp:
        tmp = Path(tmp)
        keyed, config = tmp / "keyed.elf", tmp / "block-config"
        old_key, new_key = tmp / "old-key", tmp / "new-key"
        new_config = tmp / "new-block-config"
        subprocess.run(["python3", ROOT / "experiments/ghc-microkit/provision_utm_vault.py",
                        "--objcopy", objcopy, "--source", old, "--input", compiled, "--output", keyed], check=True)
        subprocess.run([objcopy, "--dump-section", f".blk_client_config={config}", old], check=True)
        result = output / "rust_vault_disk.elf"
        subprocess.run([objcopy, "--update-section", f".blk_client_config={config}", keyed, result], check=True)
        subprocess.run([objcopy, "--dump-section", f".blk_client_config={new_config}", result], check=True)
        if config.read_bytes() != new_config.read_bytes():
            raise ValueError("rebuilt vault does not contain the original block configuration")
        subprocess.run([objcopy, "--dump-section", f".utm_vault_key={old_key}", old], check=True)
        subprocess.run([objcopy, "--dump-section", f".utm_vault_key={new_key}", result], check=True)
        if len(old_key.read_bytes()) != 32 or old_key.read_bytes() != new_key.read_bytes():
            raise ValueError("rebuilt vault does not contain the original installation key")
    return output


def build(bundle, inputs, stage, rebuild_wallet, rebuild_ui=False):
    private = json.loads((inputs / "installation.json").read_text())
    template = bundle / "config.plist"
    if not template.exists() and bundle == Path(private["bundle"]):
        template = inputs / "template.plist"
    config = plistlib.loads(template.read_bytes())
    pds = inputs / "pds"
    if config["Information"]["UUID"] != private["uuid"] or digest(pds / "rust_vault_disk.elf") != private["vault_sha256"]:
        raise ValueError("bundle and vault must belong to the prepared installation")
    for name, expected in PROFILE["release_elf_sha256"].items():
        if digest(pds / name) != expected:
            raise ValueError(f"{name} differs from the pinned {PROFILE['name']} release")

    sdk = inputs / "sdk"
    board = sdk / "board" / BOARD / CONFIG
    for name, expected in PROFILE["sdk_elf_sha256"].items():
        if digest(board / "elf" / name) != expected:
            raise ValueError(f"{name} differs from the pinned four-core SMC SDK")
    header = board / "include/kernel/gen_config.h"
    cores = PROFILE["cpus"]
    if not re.search(rf"^#define CONFIG_MAX_NUM_NODES\s+{cores}\b", header.read_text(), re.M):
        raise ValueError("guest kernel CPU count disagrees with the UTM profile")
    system = HERE / "accelerated.system"
    tree = ET.parse(system)
    workers = [int(pd.get("cpu")) for pd in tree.iter("protection_domain")
               if pd.find("program_image[@path='rust_wallet_range_worker.elf']") is not None]
    if sorted(workers) != list(range(1, cores)):
        raise ValueError("scan worker affinities disagree with the UTM profile")

    search = []
    search.extend(["--search-path", build_gpu_from_source(inputs, stage)])
    if rebuild_ui or ui_source_required(pds):
        search.extend(["--search-path", build_ui_from_source(inputs, stage)])
    if rebuild_ui or vault_source_required(pds):
        search.extend(["--search-path", build_vault_from_source(inputs, stage)])
    if rebuild_wallet:
        wallet = stage / "wallet"
        env = dict(os.environ, CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(ROOT / "build/utm-accelerated-cargo")))
        subprocess.run([ROOT / "experiments/ghc-microkit/build_wallet_scan_workers.sh",
                        inputs / "sel4-install", wallet, "--" + PROFILE["name"]], check=True, env=env)
        search.extend(["--search-path", wallet])
    subprocess.run([sdk / "bin/microkit", system, "--board", BOARD, "--config", CONFIG,
                    *search, "--search-path", pds, "-o", stage / "loader.img", "-r", stage / "loader.report"],
                   check=True, cwd=stage)
    config["System"].update(Architecture="aarch64", Target="virt", CPU="host", CPUCount=cores,
                            MemorySize=PROFILE["memory_mib"])
    config["QEMU"].update(Hypervisor=True, MachinePropertyOverride=PROFILE["machine"])
    args = config["QEMU"]["AdditionalArguments"]
    loaders = [i for i, arg in enumerate(args) if re.fullmatch(
        r'-device\s+"?loader,file=.+,addr=0x70000000,cpu-num=0"?', arg)]
    if len(loaders) != 1:
        raise ValueError("expected the regular UTM loader device")
    image = str(bundle / "Data/loader.img").replace(",", ",,")
    device = f"loader,file={image},addr=0x70000000,cpu-num=0"
    if any(ch.isspace() or ch in '\\"' for ch in device):
        device = '"' + device.replace('\\', '\\\\').replace('"', '\\"') + '"'
    args[loaders[0]] = "-device " + device
    (stage / "config.plist").write_bytes(plistlib.dumps(config, sort_keys=False))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", type=Path, help="regular UTM bundle supplying its identity and devices (default: prepared installation)")
    parser.add_argument("--inputs", type=Path, default=INPUTS)
    parser.add_argument("--output", type=Path, default=ROOT / "build/utm-accelerated", help="staging directory")
    parser.add_argument("--rebuild-wallet", action="store_true", help="compile current wallet sources using the selected release settings")
    parser.add_argument("--rebuild-ui", action="store_true", help="compile the current UI and background photo worker")
    args = parser.parse_args()
    os.umask(0o077)
    inputs, out = args.inputs.resolve(), args.output.resolve()
    if any(p.suffix.lower() == ".utm" for p in (out, *out.parents)) or out.is_relative_to(inputs):
        parser.error("choose a staging directory outside VM bundles and private inputs")
    if not (inputs / "installation.json").is_file():
        parser.error("prepare the installation's build inputs with prepare_accelerated.py first")
    bundle = (args.profile or Path(json.loads((inputs / "installation.json").read_text())["bundle"])).resolve()
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=out) as tmp:
        stage = Path(tmp)
        build(bundle, inputs, stage, args.rebuild_wallet, args.rebuild_ui)
        with (out / ".publish.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            for name in ("loader.img", "config.plist"):
                os.replace(stage / name, out / name)
    print(f"{PROFILE['name']} ({PROFILE['cpus']} HVF vCPUs) staged in {out} for {bundle.name}")


if __name__ == "__main__":
    main()
