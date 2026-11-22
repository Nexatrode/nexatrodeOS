"""Capture the matching build inputs of a verified, persistent accelerated UTM installation."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import shutil
import tempfile

from accelerated import INPUTS, PROFILE, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("manifest", type=Path, help="verified regular loader and per-ELF digests")
    parser.add_argument("sdk", type=Path, help="four-core EL1 Microkit SDK with SMC loader")
    parser.add_argument("sel4_install", type=Path, help="matching headers for wallet source builds")
    parser.add_argument("search", type=Path, nargs="+", help="directories containing the matching ELFs")
    parser.add_argument("--output", type=Path, default=INPUTS)
    args = parser.parse_args()
    os.umask(0o077)
    manifest = json.loads(args.manifest.read_text())
    if manifest.get("benchmark") != PROFILE["name"] or digest(args.bundle / "Data/loader.img") != manifest["loader_sha256"]:
        parser.error("the saved-installation manifest must match the installed loader and release")
    hashes = manifest["pd_hashes"]
    if manifest["vault_sha256"] != hashes["rust_vault_disk.elf"] or any(
        hashes[name] != expected for name, expected in PROFILE["release_elf_sha256"].items()
    ):
        parser.error("the vault or release components disagree with the manifest")
    if args.output.exists():
        parser.error(f"input directory already exists: {args.output}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.output.parent) as tmp:
        prepared = Path(tmp) / "inputs"
        pds = prepared / "pds"
        pds.mkdir(parents=True)
        for name, expected in hashes.items():
            if Path(name).name != name or not name.endswith(".elf"):
                parser.error("invalid ELF name in manifest")
            for directory in args.search:
                src = directory / name
                if src.is_file() and digest(src) == expected:
                    shutil.copyfile(src, pds / name)
                    break
            else:
                parser.error(f"matching component missing: {name}")
        shutil.copytree(args.sdk, prepared / "sdk")
        shutil.copytree(args.sel4_install, prepared / "sel4-install")
        template = (args.bundle / "config.plist").read_bytes()
        config = plistlib.loads(template)
        (prepared / "template.plist").write_bytes(template)
        identity = {"uuid": config["Information"]["UUID"], "bundle": str(args.bundle.resolve()),
                    "vault_sha256": manifest["vault_sha256"]}
        (prepared / "installation.json").write_text(json.dumps(identity) + "\n")
        prepared.rename(args.output)
    print(f"Prepared private accelerated build inputs in {args.output}")


if __name__ == "__main__":
    main()
