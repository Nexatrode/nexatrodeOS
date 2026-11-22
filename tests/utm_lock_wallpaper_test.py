#!/usr/bin/env python3
"""Check a 1080x2340 UTM lock screenshot with Aurora artwork enabled.

Usage: python3 tests/utm_lock_wallpaper_test.py path/to/screendump.ppm
The sampled area lies over the sky, away from the lock icon and text.
"""
import sys
from collections import Counter
from pathlib import Path


def verify_wallpaper(path):
    data = Path(path).read_bytes()
    header = b"P6\n1080 2340\n255\n"
    if not data.startswith(header) or len(data) != len(header) + 1080 * 2340 * 3:
        raise ValueError("Expected a 1080x2340 binary PPM screendump")
    pixels = memoryview(data)[len(header):]
    colors = Counter(
        bytes(pixels[(y * 1080 + x) * 3:(y * 1080 + x) * 3 + 3])
        for y in range(145, 230, 3) for x in range(100, 900, 3)
    )
    most_common = colors.most_common(1)[0][1] / sum(colors.values())
    if most_common >= 0.95:
        raise AssertionError("The lock wallpaper is covered by a solid band")
    print(f"PASS: lock wallpaper visible ({len(colors)} colors, dominant {most_common:.1%})")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    verify_wallpaper(sys.argv[1])
