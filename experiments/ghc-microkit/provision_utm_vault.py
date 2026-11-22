#!/usr/bin/env python3
"""Carry a provisioned UTM vault key into a newly built vault ELF."""

import argparse
import os
import subprocess
import tempfile
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--objcopy", required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    # Never print the key or leave it in a world-readable temporary file.
    with tempfile.TemporaryDirectory(prefix="nexawal-vault-key-") as temp:
        os.chmod(temp, 0o700)
        key = Path(temp) / "key"
        subprocess.run([args.objcopy, "--dump-section", f".utm_vault_key={key}", str(args.source)], check=True)
        if len(key.read_bytes()) != 32 or key.read_bytes() == bytes(32):
            raise ValueError("source vault does not contain a provisioned key")
        os.chmod(key, 0o600)
        subprocess.run([args.objcopy, "--update-section", f".utm_vault_key={key}", str(args.input), str(args.output)], check=True)


if __name__ == "__main__":
    main()
