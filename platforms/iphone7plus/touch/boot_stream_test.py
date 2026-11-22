#!/usr/bin/env python3
"""Exercise D11 boot-stream framing and reject mismatched private assets."""

import importlib.util
from pathlib import Path
import plistlib
import struct


path = Path(__file__).with_name("build-boot-stream.py")
spec = importlib.util.spec_from_file_location("boot_stream", path)
boot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boot)

assert boot.put32xe(0x10009000) == bytes.fromhex("90001000")
assert boot.register_write(0x10003060, 0xFFFFFFFF, 6099) == bytes.fromhex(
    "1e3330601000ffffffff17d300000586")
assert boot.calibration_load(0x10009000, bytes.fromhex("01020304")) == bytes.fromhex(
    "18e1300100019000100000a102010403000a0000")

config = b"""<dict>
<key>Calibration Sequence</key><array>
<dict><key>Provider</key><string>multi-touch-calibration</string><key>Address</key><integer>0x10009000</integer><key>Max Size</key><integer>8</integer></dict>
<dict><key>Provider</key><string>orb-gap-cal</string><key>Address</key><integer>0x401000</integer><key>Max Size</key><integer>8</integer></dict>
<dict><key>Provider</key><string>orb-force-cal</string><key>Address</key><integer>0x402000</integer><key>Max Size</key><integer>8</integer></dict>
<dict><key>Provider</key><string>shape-dynamic-accel-cal</string><key>Address</key><integer>0x403000</integer><key>Max Size</key><integer>8</integer></dict>
</array>
<key>Boot Sequence</key><array>
<dict><key>Address</key><integer>0x10003060</integer><key>Mask</key><integer ID="mask">0xffffffff</integer><key>Value</key><integer ID="value">6099</integer></dict>
<dict><key>Address</key><integer>0x1000305c</integer><key>Mask</key><integer IDREF="mask"/><key>Value</key><integer IDREF="value"/></dict>
<dict><key>Action</key><string>RequestCalibration</string></dict>
</array></dict>"""
firmware = plistlib.dumps({boot.PERSONALITY: {
    "Constructed Firmware": [b"abc", b"defg"],
    "Firmware Config": config,
}})
calibrations = {name: b"\x01\x02\x03\x04" for name in boot.PROVIDERS.values()}
stream, records = boot.build_stream(firmware, calibrations)
assert [kind for kind, _ in records] == [4, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3]
magic, version, count, size = struct.unpack_from("<4sIII", stream)
assert (magic, version, count, size) == (b"NTFW", 1, len(records), len(stream))
assert records[0] == (4, bytes.fromhex("02000000"))
assert records[-3] == (2, boot.register_write(0x1000305c, 0xffffffff, 6099))
assert records[-2] == (2, b"\x1f\x01")
assert records[-1] == (3, b"")

bad_reference = plistlib.dumps({boot.PERSONALITY: {
    "Constructed Firmware": [b"abc", b"defg"],
    "Firmware Config": config.replace(b'IDREF="mask"', b'IDREF="missing"'),
}})
try:
    boot.build_stream(bad_reference, calibrations)
except ValueError as error:
    assert "reference" in str(error)
else:
    raise AssertionError("missing firmware config reference accepted")

too_large = dict(calibrations)
too_large["MtCl"] = b"1234567890"
try:
    boot.build_stream(firmware, too_large)
except ValueError as error:
    assert "exceeds" in str(error)
else:
    raise AssertionError("oversize calibration accepted")

wrong_firmware = plistlib.dumps({"C1F5D,2": {
    "Constructed Firmware": [b"abc", b"defg"],
    "Firmware Config": config,
}})
try:
    boot.build_stream(wrong_firmware, calibrations)
except ValueError as error:
    assert "personality" in str(error)
else:
    raise AssertionError("wrong firmware personality accepted")

print("D11 touch boot stream: PASS")
