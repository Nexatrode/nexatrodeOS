#!/usr/bin/env python3
"""Prefix a Microkit loader.img with an arm64 Linux Image header page.

The header requests text_offset 0x80000; the native large-image snapshot
observed entry at 0xb0080000. Instruction zero branches over the 4 KiB header
page into the Microkit loader. The loader
must retain its 4 KiB link alignment: it uses ADRP/ADD page-relative pairs
while relocating itself before the MMU is enabled.
This output is only for the temporary boot-v2 packer, not direct flashing.
"""

import argparse
from pathlib import Path
import struct


HEADER_PAGE = 4096


def wrap(loader: bytes) -> bytes:
    # The stack page displacement changes when visible loader markers are
    # added. Pin the instruction class and destination register: ADRP x1.
    if len(loader) < 64 or struct.unpack_from('<I', loader)[0] & 0x9f00001f != 0x90000001:
        raise ValueError("unexpected Microkit AArch64 loader prefix")
    header = bytearray(HEADER_PAGE)
    struct.pack_into("<I", header, 0, 0x14000400)  # b +4096
    struct.pack_into("<Q", header, 8, 0x80000)     # text_offset
    struct.pack_into("<Q", header, 16, HEADER_PAGE + len(loader))
    struct.pack_into("<Q", header, 24, 0x0A)       # LE, 4K physical pages
    struct.pack_into("<I", header, 56, 0x644D5241)  # ARM\x64
    return bytes(header) + loader


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("loader", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.write_bytes(wrap(args.loader.read_bytes()))
    print(f"wrote {args.output} ({args.output.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
