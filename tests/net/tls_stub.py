#!/usr/bin/env python3
# Copyright 2026, Personal OS contributors
# SPDX-License-Identifier: BSD-2-Clause
"""TLS 1.2 PSK-AES128-CCM8 echo stub for ./dev test-net (port 1239).

Demo PSK only — not for production. Matches tests/net/tls_smoke.c.
"""

from __future__ import annotations

import argparse
import socket
import ssl
import sys

PSK_IDENTITY = "personal-os"
PSK = bytes(
    (
        0x50, 0x65, 0x72, 0x73, 0x6F, 0x6E, 0x61, 0x6C,
        0x4F, 0x53, 0x2D, 0x54, 0x4C, 0x53, 0x21, 0x21,
    )
)
CIPHER = "PSK-AES128-CCM8:@SECLEVEL=0"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--listen", type=int, default=1239)
    parser.add_argument("--count", type=int, default=1,
                        help="Exit after this many successful echoes (0 = forever)")
    args = parser.parse_args()

    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.maximum_version = ssl.TLSVersion.TLSv1_2
    ctx.minimum_version = ssl.TLSVersion.TLSv1_2
    ctx.set_ciphers(CIPHER)

    def psk_cb(identity: str) -> bytes | None:
        if identity == PSK_IDENTITY:
            return PSK
        return None

    ctx.set_psk_server_callback(psk_cb)

    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("0.0.0.0", args.listen))
    sock.listen(5)
    sock.settimeout(1.0)
    print(f"tls_stub: listening on 0.0.0.0:{args.listen}", flush=True)

    answered = 0
    while args.count == 0 or answered < args.count:
        try:
            conn, addr = sock.accept()
        except socket.timeout:
            continue
        try:
            with ctx.wrap_socket(conn, server_side=True) as tls:
                data = tls.recv(256)
                if data:
                    tls.sendall(data)
                    answered += 1
                    print(f"tls_stub: echoed {len(data)} bytes for {addr}", flush=True)
        except ssl.SSLError as error:
            print(f"tls_stub: ssl error from {addr}: {error}", flush=True)
        finally:
            try:
                conn.close()
            except OSError:
                pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
