#!/usr/bin/env python3
"""Embed the QEMU-only test CA in the statically linked Haskell TLS probe."""

from pathlib import Path
import sys


def main() -> None:
    source = Path(sys.argv[1])
    destination = Path(sys.argv[2])
    module = sys.argv[3] if len(sys.argv) > 3 else "TLSFixtureCA"
    if not module.isidentifier():
        raise SystemExit("invalid Haskell fixture module name")
    data = source.read_bytes()
    if not data or len(data) > 8192:
        raise SystemExit("unexpected QEMU test CA size")
    body = ", ".join(str(byte) for byte in data)
    destination.write_text(
        "-- Generated QEMU test fixture. Never use this CA as a production root.\n"
        f"module {module} (caDER) where\n"
        "import qualified Data.ByteString as B\n"
        "caDER :: B.ByteString\n"
        f"caDER = B.pack [{body}]\n"
    )


if __name__ == "__main__":
    main()
