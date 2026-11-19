#!/usr/bin/env python3
"""Validate and extract only the D11 touch records from a local SysCfg dump."""

import argparse
import hashlib
import os
from pathlib import Path
import struct
import sys


RECORDS = ("MtCl", "OrbG", "OFCl", "SDAC")
HEADER_SIZE = 24
ENTRY_SIZE = 20


def read_records(data: bytes) -> dict[str, bytes]:
    if len(data) < HEADER_SIZE or data[:4] != b"gfCS":
        raise ValueError("invalid SysCfg header")
    declared_size = struct.unpack_from("<I", data, 8)[0]
    count = struct.unpack_from("<I", data, 20)[0]
    if declared_size > len(data) or declared_size < HEADER_SIZE or count > (declared_size - HEADER_SIZE) // ENTRY_SIZE:
        raise ValueError("SysCfg index is out of bounds")
    found = {}
    for index in range(count):
        entry = data[HEADER_SIZE + index * ENTRY_SIZE:HEADER_SIZE + (index + 1) * ENTRY_SIZE]
        if entry[:4] == b"BTNC":
            name = entry[4:8][::-1].decode("ascii", errors="replace")
            size, offset = struct.unpack_from("<II", entry, 8)
            if name not in RECORDS:
                continue
            if size == 0 or size > declared_size or offset > declared_size - size:
                raise ValueError(f"out-of-bounds {name} record")
            value = data[offset:offset + size]
        else:
            name = entry[:4][::-1].decode("ascii", errors="replace")
            if name not in RECORDS:
                continue
            value = entry[4:20]
        if name in found:
            raise ValueError(f"duplicate {name} record")
        found[name] = value
    missing = [name for name in RECORDS if name not in found]
    if missing:
        raise ValueError("missing SysCfg records: " + ", ".join(missing))
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("syscfg_dump", type=Path)
    parser.add_argument("--output-dir", type=Path,
                        help="write four calibration files; refuses to overwrite")
    args = parser.parse_args()
    try:
        records = read_records(args.syscfg_dump.read_bytes())
        if args.output_dir:
            args.output_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
            for name in RECORDS:
                if (args.output_dir / (name + ".bin")).exists():
                    raise ValueError(f"refusing to overwrite {name}.bin")
            for name, value in records.items():
                path = args.output_dir / (name + ".bin")
                fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                with os.fdopen(fd, "wb") as output:
                    output.write(value)
        for name in RECORDS:
            value = records[name]
            print(f"{name}: {len(value)} bytes, SHA-256 {hashlib.sha256(value).hexdigest()}")
    except (OSError, ValueError) as error:
        print(f"SysCfg calibration extraction failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
