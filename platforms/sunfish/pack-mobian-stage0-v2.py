#!/usr/bin/env python3
"""Use the phone's proven temporary v2 wrapper for the comparison payload.

Unlike the community installed v0 recipe, this retains the vendor DTB for ABL
and puts the U-Boot control DTB inside gzip, exactly as U-Boot's OF_SEPARATE
u-boot.bin concatenation does. It is a handoff experiment, not a flash image.
"""

import argparse
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / ".cache/sunfish-mobian-comparison/artifacts"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    args = parser.parse_args()
    helper = Path(__file__).with_name("repack-boot-v2.py")
    spec = importlib.util.spec_from_file_location("sunfish_boot_v2", helper)
    packer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(packer)
    reference = args.reference.read_bytes()
    header, original, _ = packer.components(reference)
    combined = (OUT / "u-boot-nodtb.bin").read_bytes() + (OUT / "uboot-sunfish.dtb").read_bytes()
    payload = gzip.compress(combined, compresslevel=9, mtime=0)
    disk = (OUT / "comparison-esp.img").read_bytes()
    assert len(disk) == 64 * 1024**2
    image = packer.pack(reference, payload, disk)
    actual_header, actual, _ = packer.components(image)
    assert gzip.decompress(actual["kernel"]) == combined
    assert actual["ramdisk"] == disk and actual["dtb"] == original["dtb"]
    for offset, size in [(12, 4), (20, 4), (28, 20), (48, 528), (608, 1024), (1648, 12)]:
        assert actual_header[offset:offset+size] == header[offset:offset+size]
    target = OUT / "sunfish-linux-comparison-stage0-v2.img"
    target.write_bytes(image)
    sha = lambda data: hashlib.sha256(data).hexdigest()
    record = {
        "kind": "temporary_boot_v2_handoff_experiment",
        "hardware_boot_verified": False,
        "boot_header_version": 2,
        "image_bytes": len(image),
        "image_sha256": sha(image),
        "reference_sha256": sha(reference),
        "vendor_dtb_sha256": sha(original["dtb"]),
        "kernel_layout": "gzip(u-boot-nodtb.bin + U-Boot control DTB); separate vendor DTB retained for ABL",
        "inputs": {name: sha((OUT/name).read_bytes()) for name in
                   ["u-boot-nodtb.bin", "uboot-sunfish.dtb", "comparison-esp.img", "stage0-boot-manifest.json"]},
        "helpers": {str(p.relative_to(ROOT)): sha(p.read_bytes()) for p in [helper, Path(__file__)]},
        "radio_services": "held; no firmware payload",
    }
    (OUT / "stage0-v2-boot-manifest.json").write_text(json.dumps(record, indent=2) + "\n")
    print(f"PASS: v2 temporary handoff image packed and round-tripped ({len(image)} bytes)")
    print(f"SHA256: {record['image_sha256']}")


if __name__ == "__main__":
    main()
