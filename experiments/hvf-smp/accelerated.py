"""Build the regular four-core UTM image using the measured NexaWal release."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
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


def build(bundle, inputs, stage, rebuild_wallet):
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
        build(bundle, inputs, stage, args.rebuild_wallet)
        with (out / ".publish.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            for name in ("loader.img", "config.plist"):
                os.replace(stage / name, out / name)
    print(f"{PROFILE['name']} ({PROFILE['cpus']} HVF vCPUs) staged in {out} for {bundle.name}")


if __name__ == "__main__":
    main()
