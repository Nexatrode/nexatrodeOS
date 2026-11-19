#!/usr/bin/env python3
"""Replace the kernel in a sunfish Android boot-v2 image for *temporary* boot.

The source supplies its DTB, addresses, cmdline and, by default, ramdisk.
No AVB footer is copied: the output is intended only for ``fastboot boot`` on
this unlocked development phone, never for flashing a partition.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import struct


def u32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def align(value: int, page: int) -> int:
    return (value + page - 1) & ~(page - 1)


def components(image: bytes) -> tuple[bytearray, dict[str, bytes], int]:
    if image[:8] != b"ANDROID!" or u32(image, 40) != 2:
        raise ValueError("source must be an Android boot header v2 image")
    page = u32(image, 36)
    if page < 1660 or page & (page - 1) or u32(image, 1644) != 1660:
        raise ValueError("unexpected boot header/page size")
    sizes = {
        "kernel": u32(image, 8),
        "ramdisk": u32(image, 16),
        "second": u32(image, 24),
        "recovery_dtbo": u32(image, 1632),
        "dtb": u32(image, 1648),
    }
    if not sizes["kernel"] or not sizes["ramdisk"] or not sizes["dtb"]:
        raise ValueError("source lacks required v2 components")
    if sizes["second"] or sizes["recovery_dtbo"]:
        raise ValueError("this packer only supports sunfish's simple v2 layout")
    pos = page
    parts = {}
    for name, size in sizes.items():
        parts[name] = image[pos : pos + size]
        if len(parts[name]) != size:
            raise ValueError(f"truncated {name}")
        pos += align(size, page)
    if parts["dtb"][:4] != b"\xd0\x0d\xfe\xed":
        raise ValueError("DTB does not start with FDT magic")
    return bytearray(image[:page]), parts, page


def pack(
    reference: bytes,
    kernel: bytes,
    ramdisk: bytes | None = None,
    append_cmdline: str | bytes | None = None,
) -> bytes:
    header, parts, page = components(reference)
    if not kernel:
        raise ValueError("empty replacement kernel")
    if append_cmdline is not None:
        if isinstance(append_cmdline, str):
            try:
                addition = append_cmdline.encode("ascii")
            except UnicodeEncodeError as error:
                raise ValueError("appended kernel command line must be ASCII") from error
        else:
            addition = bytes(append_cmdline)
        if not addition or b"\0" in addition:
            raise ValueError("appended kernel command line must be nonempty and NUL-free")

        base_field = bytes(header[64:576])
        extra_field = bytes(header[608:1632])
        if b"\0" not in base_field or b"\0" not in extra_field:
            raise ValueError("source kernel command line is not NUL-terminated")
        base = base_field.split(b"\0", 1)[0]
        extra = extra_field.split(b"\0", 1)[0]
        existing = base + extra
        separator = (
            b" " if existing and not existing.endswith((b" ", b"\t"))
            and not addition.startswith((b" ", b"\t")) else b""
        )
        extra += separator + addition
        if len(extra) >= len(extra_field):
            raise ValueError("appended kernel command line exceeds extra_cmdline capacity")
        if len(base) + len(extra) >= len(base_field) + len(extra_field):
            raise ValueError("appended kernel command line exceeds total v2 capacity")
        header[608:1632] = extra + b"\0" * (len(extra_field) - len(extra))

    parts["kernel"] = kernel
    struct.pack_into("<I", header, 8, len(kernel))
    if ramdisk is not None:
        if not ramdisk:
            raise ValueError("replacement ramdisk must be nonempty")
        parts["ramdisk"] = ramdisk
        struct.pack_into("<I", header, 16, len(ramdisk))
    sha = hashlib.sha1()
    for name in ("kernel", "ramdisk", "second", "recovery_dtbo", "dtb"):
        data = parts[name]
        sha.update(data)
        sha.update(struct.pack("<I", len(data)))
    header[576:608] = sha.digest().ljust(32, b"\0")
    result = bytearray(header)
    for name in ("kernel", "ramdisk", "second", "recovery_dtbo", "dtb"):
        result.extend(parts[name])
        result.extend(b"\0" * (align(len(parts[name]), page) - len(parts[name])))
    return bytes(result)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("kernel", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--append-cmdline", help="append one ASCII argument string to the kernel cmdline")
    parser.add_argument("--ramdisk", type=Path,
                        help="optional minimal ramdisk for native payloads that do not use Android")
    args = parser.parse_args()
    ramdisk = args.ramdisk.read_bytes() if args.ramdisk is not None else None
    output = pack(
        args.reference.read_bytes(), args.kernel.read_bytes(), ramdisk,
        append_cmdline=args.append_cmdline,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(output)
    _, parts, _ = components(output)
    assert parts["kernel"] == args.kernel.read_bytes()
    if ramdisk is not None:
        assert parts["ramdisk"] == ramdisk
    print(f"wrote {args.output} ({len(output)} bytes; temporary boot only)")


if __name__ == "__main__":
    main()
