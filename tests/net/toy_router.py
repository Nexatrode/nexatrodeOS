#!/usr/bin/env python3
"""Forward opaque UDP datagrams from a listen port to a peer hostfwd port."""
from __future__ import annotations

import argparse
import socket
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--listen", type=int, default=1237)
    parser.add_argument("--forward", type=int, default=1246)
    parser.add_argument("--count", type=int, default=1, help="Stop after N datagrams (0=forever)")
    parser.add_argument("--save", type=Path, help="Write the last datagram to this path")
    args = parser.parse_args()

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(("0.0.0.0", args.listen))
        forwarded = 0
        last = b""
        while args.count == 0 or forwarded < args.count:
            data, _address = sock.recvfrom(2048)
            sock.sendto(data, ("127.0.0.1", args.forward))
            last = data
            forwarded += 1
            print(f"toy_router: forwarded {len(data)} bytes to :{args.forward}", flush=True)
        if args.save is not None:
            args.save.write_bytes(last)
    return 0


if __name__ == "__main__":
    sys.exit(main())
