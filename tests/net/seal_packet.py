#!/usr/bin/env python3
"""Load the fixed PSL1 UDP demo datagram for ./dev test-net."""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PACKET_BIN = HERE / "psl1_demo_packet.bin"
SEAL_HOST = HERE / "seal_host"


def demo_packet() -> bytes:
    if not PACKET_BIN.exists():
        subprocess.run(
            ["cargo", "run", "--quiet", "--", str(PACKET_BIN)],
            check=True,
            cwd=SEAL_HOST,
        )
    return PACKET_BIN.read_bytes()


def main() -> None:
    sys.stdout.buffer.write(demo_packet())


if __name__ == "__main__":
    main()
