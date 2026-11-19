#!/usr/bin/env python3
"""Package the audited U-Boot/RAM ESP candidate in the reference Android v0 layout."""

import gzip
import hashlib
import json
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / ".cache/sunfish-mobian-comparison/artifacts"
PAGE = 4096


def padded(data):
    return data + bytes((-len(data)) % PAGE)


def main():
    uboot = (OUT / "u-boot-nodtb.bin").read_bytes()
    dtb = (OUT / "uboot-sunfish.dtb").read_bytes()
    disk = (OUT / "comparison-esp.img").read_bytes()
    if uboot[56:60] != b"ARM\x64" or struct.unpack_from("<Q", uboot, 8)[0] != 0x80000:
        raise SystemExit("U-Boot lacks the pinned Sunfish ARM64 header/text offset")
    if dtb[:4] != b"\xd0\x0d\xfe\xed" or struct.unpack_from(">I", dtb, 4)[0] != len(dtb):
        raise SystemExit("Invalid U-Boot control DTB")
    if len(disk) != 64 * 1024**2 or disk[512:520] != b"EFI PART":
        raise SystemExit("Invalid RAM ESP disk image")
    kernel = gzip.compress(uboot, compresslevel=9, mtime=0) + dtb
    identifier = hashlib.sha1()
    for part in [kernel, disk, b""]:
        identifier.update(part)
        identifier.update(struct.pack("<I", len(part)))
    os_version = (13 << 25) | ((2023 - 2000) << 4) | 8
    header = b"ANDROID!" + struct.pack(
        "<10I", len(kernel), 0x8000, len(disk), 0x1000000,
        0, 0xF00000, 0x100, PAGE, 0, os_version,
    ) + bytes(16 + 512) + identifier.digest().ljust(32, b"\0") + bytes(1024)
    image = padded(header) + padded(kernel) + padded(disk)
    if len(image) >= 256 * 1024**2:
        raise SystemExit("Candidate exceeds the conservative 256 MiB download bound")
    # Round-trip the actual container/header boundaries before writing an artifact.
    decoded = zlib.decompressobj(16 + zlib.MAX_WBITS)
    if decoded.decompress(kernel) != uboot or decoded.unused_data != dtb:
        raise SystemExit("U-Boot gzip/DTB round trip failed")
    target = OUT / "sunfish-linux-comparison-stage0.img"
    target.write_bytes(image)
    record = {
        "kind": "temporary_boot_transport_candidate",
        "hardware_boot_verified": False,
        "chain": "ABL -> pinned U-Boot -> RAM ESP -> systemd-boot -> Linux EFI -> staging initramfs",
        "uboot_commit": "f8c04469e8284aa2f1691a30aa3b7dc59fa0b8cd",
        "boot_header_version": 0,
        "page_size": PAGE,
        "second_addr": "0x00f00000",
        "image_bytes": len(image),
        "image_sha256": hashlib.sha256(image).hexdigest(),
        "inputs": {name: hashlib.sha256((OUT/name).read_bytes()).hexdigest()
                   for name in ["u-boot-nodtb.bin", "uboot-sunfish.dtb", "uboot.config", "comparison-esp.img",
                                "vmlinuz.efi", "comparison-stage0.cpio.xz", "sm7150-google-sunfish.dtb",
                                "BOOTAA64.EFI", "efi-package.txt", "efi-package.sha256",
                                "uboot-toolchain-packages.tsv", "uboot-compiler.txt"]},
        "build_helpers": {name: hashlib.sha256((ROOT/name).read_bytes()).hexdigest()
                          for name in ["platforms/sunfish/build-mobian-boot-stage0.sh",
                                       "platforms/sunfish/mobian-comparison/build-boot.sh",
                                       "platforms/sunfish/mobian-comparison/ram-esp.env",
                                       "platforms/sunfish/mobian-comparison/ram-esp-reserve.c",
                                       "platforms/sunfish/pack-mobian-stage0.py"]},
        "ram_esp_blocks": "0x20000",
        "ram_esp_address": "prevbl_initrd_start_addr from ABL's FDT; no guessed physical address",
        "ram_esp_reservation": "Board hook validates ABL initrd start/end and reserves 64 MiB in LMB before board payload allocations; EFI is notified",
        "persistent_write_features": "Fastboot flashing/OEM run, EFI capsule updates and EFI file-variable storage disabled",
        "radio_services": "held in staging userspace; no peripheral firmware payload",
    }
    (OUT / "stage0-boot-manifest.json").write_text(json.dumps(record, indent=2) + "\n")
    print(f"PASS: candidate packaged ({len(image)} bytes), Android v0 header and gzip/DTB round trip checked")
    print(f"SHA256: {record['image_sha256']}")
    print("LIMIT: actual ABL RAM-disk handoff and RAM-ESP EFI boot still require a Sunfish check")


if __name__ == "__main__":
    main()
