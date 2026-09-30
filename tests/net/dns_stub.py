#!/usr/bin/env python3
# Copyright 2026, Personal OS contributors
# SPDX-License-Identifier: BSD-2-Clause
"""Authoritative stub: personal-os.test → 10.0.2.2 on UDP (default 53535).

Used by ./dev test-net so the guest lwIP DNS client does not need host port 53
or external resolvers. Matches DNS_SERVER_PORT in tests/net lwipopts.
Avoids macOS mDNS on 5353.
"""

from __future__ import annotations

import argparse
import socket
import struct
import sys


EXPECTED_LABELS = (b"personal-os", b"test")
ANSWER_IP = bytes((10, 0, 2, 2))


def skip_name(data: bytes, pos: int) -> int:
    while pos < len(data):
        length = data[pos]
        if length == 0:
            return pos + 1
        if length & 0xC0:
            return pos + 2
        pos += 1 + length
    raise ValueError("truncated DNS name")


def question_matches(data: bytes) -> bool:
    if len(data) < 12:
        return False
    pos = 12
    labels: list[bytes] = []
    while pos < len(data):
        length = data[pos]
        if length == 0:
            pos += 1
            break
        if length & 0xC0:
            return False
        pos += 1
        labels.append(data[pos : pos + length])
        pos += length
    if pos + 4 > len(data):
        return False
    qtype, qclass = struct.unpack("!HH", data[pos : pos + 4])
    return labels == list(EXPECTED_LABELS) and qtype == 1 and qclass == 1


def build_response(query: bytes) -> bytes | None:
    if not question_matches(query):
        return None
    tid = query[:2]
    flags = b"\x85\x00"  # response, authoritative, no recursion
    counts = struct.pack("!HHHH", 1, 1, 0, 0)
    qend = skip_name(query, 12) + 4
    question = query[12:qend]
    answer = b"\xc0\x0c" + struct.pack("!HHIH", 1, 1, 60, 4) + ANSWER_IP
    return tid + flags + counts + question + answer


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--listen", type=int, default=53535)
    parser.add_argument("--count", type=int, default=1,
                        help="Exit after this many successful answers (0 = forever)")
    args = parser.parse_args()

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("0.0.0.0", args.listen))
    answered = 0
    print(f"dns_stub: listening on 0.0.0.0:{args.listen}", flush=True)
    while args.count == 0 or answered < args.count:
        data, addr = sock.recvfrom(512)
        response = build_response(data)
        if response is None:
            print(f"dns_stub: ignored query from {addr}", flush=True)
            continue
        sock.sendto(response, addr)
        answered += 1
        print(f"dns_stub: answered personal-os.test for {addr}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
