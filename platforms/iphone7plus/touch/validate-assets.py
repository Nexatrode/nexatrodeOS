#!/usr/bin/env python3
"""Check D11 touch asset identity without copying firmware into the repository."""

import argparse
import hashlib
import json
import plistlib
from pathlib import Path
import sys
import xml.etree.ElementTree as ET


EXPECTED_PROVIDERS = (
    "multi-touch-calibration",
    "orb-gap-cal",
    "orb-force-cal",
    "shape-dynamic-accel-cal",
)


def validate(device_tree_path: Path, firmware_path: Path) -> None:
    tree = json.loads(device_tree_path.read_text())["device-tree"]
    if tree.get("model") != "iPhone9,2" or "D11AP" not in tree.get("compatible", []):
        raise ValueError("DeviceTree is not for iPhone9,2 / D11AP")

    arm_io = next(child["arm-io"] for child in tree["children"] if "arm-io" in child)
    spi2 = next(child["spi2"] for child in arm_io["children"] if "spi2" in child)
    touch = next(child["multi-touch"] for child in spi2["children"] if "multi-touch" in child)
    personality = touch["hid-merge-personality"]
    if personality != "C1F5E,2" or touch.get("compatible") != "hid-transport,k1":
        raise ValueError("unexpected D11 touch controller personality or transport")

    data = firmware_path.read_bytes()
    firmware = plistlib.loads(data)
    if list(firmware) != [personality]:
        raise ValueError("firmware personalities do not match the D11 DeviceTree")
    config = firmware[personality]
    blobs = config.get("Constructed Firmware")
    if not isinstance(blobs, list) or not blobs or not all(isinstance(b, bytes) for b in blobs):
        raise ValueError("firmware is missing constructed payloads")
    fragment = config["Firmware Config"].rstrip(b"\0")
    root = ET.fromstring(fragment)
    providers = []
    for entry in root.iter("dict"):
        children = list(entry)
        for index, key in enumerate(children[:-1]):
            if key.tag == "key" and key.text == "Provider":
                providers.append(children[index + 1].text)
    if tuple(providers) != EXPECTED_PROVIDERS:
        raise ValueError(f"unexpected firmware calibration providers: {providers}")

    print(f"D11 firmware OK: {personality}, {len(blobs)} constructed payloads")
    print("Required SysCfg records: MtCl OrbG OFCl SDAC")
    print(f"Firmware size: {len(data)} bytes")
    print(f"Firmware SHA-256: {hashlib.sha256(data).hexdigest()}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("device_tree_json", type=Path)
    parser.add_argument("d11_mtprops", type=Path)
    args = parser.parse_args()
    try:
        validate(args.device_tree_json, args.d11_mtprops)
    except (OSError, KeyError, ValueError, ET.ParseError) as error:
        print(f"D11 touch asset validation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
