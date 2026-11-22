#!/usr/bin/env python3
"""Capture Pixel USB-Cereal at 115200 using one open file descriptor.

Opening the macOS serial device again resets its speed to 9600 on this host.
Set and verify termios on the same descriptor used for all reads. Raw output
can include device identifiers; keep it outside the repository.
"""

import argparse
import os
from pathlib import Path
import select
import termios
import time
import tty


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--markers", type=Path,
                        help="write monotonic receipt times for SUNFISH marker lines")
    parser.add_argument("--device", default="/dev/cu.usbserial-0001")
    parser.add_argument("--seconds", type=int, default=180)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        parser.error("raw UART output must be outside the repository")
    if args.markers and args.markers.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        parser.error("marker timings must be outside the repository")

    fd = os.open(args.device, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    try:
        tty.setraw(fd)
        attrs = termios.tcgetattr(fd)
        attrs[4] = attrs[5] = termios.B115200
        attrs[2] |= termios.CLOCAL | termios.CREAD
        termios.tcsetattr(fd, termios.TCSANOW, attrs)
        actual = termios.tcgetattr(fd)
        if actual[4] != termios.B115200 or actual[5] != termios.B115200:
            raise RuntimeError("serial speed did not remain at 115200")
        print(f"capturing {args.device} at 115200 to {args.output}", flush=True)
        deadline = time.monotonic() + args.seconds
        marker_line = bytearray()
        marker_output = args.markers.open("w") if args.markers else None
        try:
            with args.output.open("wb") as output:
                while time.monotonic() < deadline:
                    readable, _, _ = select.select([fd], [], [], 1)
                    if readable:
                        data = os.read(fd, 4096)
                        if data:
                            output.write(data)
                            output.flush()
                            if marker_output:
                                for byte in data:
                                    if byte == 10:
                                        if b"SUNFISH" in marker_line:
                                            line = marker_line.decode("latin1", errors="replace").strip()
                                            marker_output.write(f"{time.monotonic_ns()} {line}\n")
                                            marker_output.flush()
                                        marker_line.clear()
                                    elif len(marker_line) < 4096:
                                        marker_line.append(byte)
        finally:
            if marker_output:
                marker_output.close()
    finally:
        os.close(fd)


if __name__ == "__main__":
    main()
