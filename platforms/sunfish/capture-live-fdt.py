#!/usr/bin/env python3
"""Capture the Pixel 4a's active device tree through read-only ADB."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import struct
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT = ROOT / ".cache/pixel-sunfish/live-lineage-current.dtb"


def adb(serial: str | None, *args: str) -> bytes:
    command = ["adb"]
    if serial:
        command += ["-s", serial]
    return subprocess.check_output(command + list(args))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", help="ADB serial when more than one device is attached")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--root-read-only",
        action="store_true",
        help="temporarily run adbd as root to read the protected FDT, then restore shell mode",
    )
    args = parser.parse_args()

    serial = args.serial
    if serial is None:
        devices = subprocess.check_output(["adb", "devices"], text=True).splitlines()[1:]
        connected = [line.split()[0] for line in devices if len(line.split()) >= 2 and line.split()[1] == "device"]
        if len(connected) != 1:
            parser.error(f"expected one authorized ADB device, found {len(connected)}; pass --serial")
        serial = connected[0]

    rooted = False
    if args.root_read_only:
        result = adb(serial, "root").decode(errors="replace")
        if "cannot run as root" in result.lower():
            raise SystemExit(result.strip())
        adb(serial, "wait-for-device")
        identity = adb(serial, "shell", "id").decode(errors="replace")
        if "uid=0" not in identity:
            raise SystemExit(f"adb root did not provide root access: {identity.strip()}")
        rooted = True

    try:
        product = adb(serial, "shell", "getprop", "ro.product.device").decode().strip()
        if product != "sunfish":
            parser.error(f"connected device identifies as {product!r}, expected Pixel 4a sunfish")

        blob = adb(serial, "exec-out", "cat", "/sys/firmware/fdt")
        if len(blob) < 8 or blob[:4] != bytes.fromhex("d00dfeed"):
            raise SystemExit("device returned data without a valid flattened-device-tree header")
        total_size = struct.unpack_from(">I", blob, 4)[0]
        if total_size != len(blob):
            raise SystemExit(f"FDT header size {total_size} does not match captured size {len(blob)}")

        output = args.output.resolve()
        output.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=output.parent, prefix=output.name + ".", delete=False) as temp:
            temp.write(blob)
            temporary = Path(temp.name)
        temporary.replace(output)
        print(f"device=sunfish serial={serial}")
        print(f"bytes={len(blob)} sha256={hashlib.sha256(blob).hexdigest()}")
        print(f"output={output}")
    finally:
        if rooted:
            adb(serial, "unroot")
            adb(serial, "wait-for-device")


if __name__ == "__main__":
    main()
