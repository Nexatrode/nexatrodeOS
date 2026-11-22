#!/usr/bin/env python3
"""Build a private D11 touch boot stream from matching firmware and calibration."""

import argparse
import base64
import binascii
import hashlib
import os
from pathlib import Path
import plistlib
import struct
import sys
import xml.etree.ElementTree as ET


PERSONALITY = "C1F5E,2"
PROVIDERS = {
    "multi-touch-calibration": "MtCl",
    "orb-gap-cal": "OrbG",
    "orb-force-cal": "OFCl",
    "shape-dynamic-accel-cal": "SDAC",
}
SET_TYPE, WRITE, WRITE_ACK, WAIT_IRQ = 4, 1, 2, 3
MAX_CONFIG = 1024 * 1024
MAX_BLOB = 1024 * 1024
MAX_RECORDS = 64


def parse_node(node, ids, active=frozenset()):
    if "IDREF" in node.attrib:
        reference = node.attrib["IDREF"]
        target = ids.get(reference)
        if target is None or target.tag != node.tag or reference in active:
            raise ValueError("invalid firmware config reference")
        return parse_node(target, ids, active | {reference})
    if node.tag == "dict":
        children = list(node)
        if len(children) % 2:
            raise ValueError("odd firmware dictionary")
        result = {}
        for index in range(0, len(children), 2):
            key = children[index]
            if key.tag != "key" or not key.text or key.text in result:
                raise ValueError("invalid firmware dictionary key")
            result[key.text] = parse_node(children[index + 1], ids, active)
        return result
    if node.tag == "array":
        return [parse_node(child, ids, active) for child in node]
    if node.tag == "integer":
        value = int(node.text or "0", 0)
        if value < 0 or value > 0xFFFFFFFFFFFFFFFF:
            raise ValueError("firmware integer out of range")
        return value
    if node.tag == "string":
        return node.text or ""
    if node.tag == "data":
        return base64.b64decode("".join((node.text or "").split()), validate=True)
    if node.tag == "true":
        return True
    if node.tag == "false":
        return False
    raise ValueError(f"unsupported firmware element: {node.tag}")


def put32xe(value):
    if type(value) is not int or not 0 <= value <= 0xFFFFFFFF:
        raise ValueError("touch register value out of range")
    return bytes((value >> 8 & 0xFF, value & 0xFF,
                  value >> 24 & 0xFF, value >> 16 & 0xFF))


def register_write(address, mask, value):
    body = put32xe(address) + put32xe(mask) + put32xe(value)
    return b"\x1e\x33" + body + struct.pack(">H", sum(body) & 0xFFFF)


def calibration_load(address, data):
    if not data or len(data) > 0xFFFF or len(data) % 2:
        raise ValueError("invalid calibration length")
    padded = (len(data) + 3) & ~3
    count = (len(data) + 3) >> 2
    header = struct.pack(">H", count) + put32xe(address)
    swapped = bytearray(padded)
    for index, byte in enumerate(data):
        swapped[index ^ 1] = byte
    return (put32xe(0x300118E1) + header +
            struct.pack(">H", sum(header) & 0xFFFF) + swapped +
            put32xe(sum(data) & 0xFFFFFFFF))


def load_config(firmware):
    if not isinstance(firmware, dict) or list(firmware) != [PERSONALITY]:
        raise ValueError("firmware personality is not D11 C1F5E,2")
    entry = firmware[PERSONALITY]
    if not isinstance(entry, dict):
        raise ValueError("invalid firmware entry")
    blobs = entry.get("Constructed Firmware")
    if not isinstance(blobs, list) or len(blobs) != 2 or any(
        not isinstance(blob, bytes) or not blob or len(blob) > MAX_BLOB
        for blob in blobs
    ):
        raise ValueError("expected two bounded constructed firmware blobs")
    fragment = entry.get("Firmware Config")
    if not isinstance(fragment, bytes) or not 0 < len(fragment) <= MAX_CONFIG:
        raise ValueError("missing or oversized firmware config")
    fragment = fragment.rstrip(b"\0")
    if b"<!DOCTYPE" in fragment.upper() or b"<!ENTITY" in fragment.upper():
        raise ValueError("firmware config contains XML declarations")
    root = ET.fromstring(fragment)
    ids = {}
    for node in root.iter():
        identifier = node.attrib.get("ID")
        if identifier is not None:
            if identifier in ids or "IDREF" in node.attrib:
                raise ValueError("duplicate or invalid firmware config ID")
            ids[identifier] = node
    config = parse_node(root, ids)
    if not isinstance(config, dict):
        raise ValueError("invalid firmware config")
    return blobs, config


def build_stream(mtprops: bytes, calibrations: dict[str, bytes]):
    blobs, config = load_config(plistlib.loads(mtprops))
    records = [(SET_TYPE, struct.pack("<I", 2)),
               (WRITE, b"\x1a\xa1\x18\xe1")]
    records.extend((WRITE_ACK, blob) for blob in blobs)

    sequence = config.get("Calibration Sequence")
    if not isinstance(sequence, list) or len(sequence) != len(PROVIDERS):
        raise ValueError("unexpected calibration sequence")
    seen = set()
    for step in sequence:
        if not isinstance(step, dict):
            raise ValueError("invalid calibration step")
        provider = step.get("Provider")
        if provider not in PROVIDERS or provider in seen:
            raise ValueError("unknown or duplicate calibration provider")
        seen.add(provider)
        data = calibrations[PROVIDERS[provider]]
        maximum = step.get("Max Size")
        if type(maximum) is not int or len(data) > maximum:
            raise ValueError(f"{provider} exceeds firmware maximum")
        records.append((WRITE_ACK, calibration_load(step.get("Address"), data)))
    if seen != set(PROVIDERS):
        raise ValueError("missing calibration provider")

    sequence = config.get("Boot Sequence")
    if not isinstance(sequence, list) or not sequence:
        raise ValueError("missing boot sequence")
    for step in sequence:
        if not isinstance(step, dict):
            raise ValueError("invalid boot step")
        if "Action" in step:
            if step != {"Action": "RequestCalibration"}:
                raise ValueError("unknown boot action")
            records.append((WRITE_ACK, b"\x1f\x01"))
        else:
            if not {"Address", "Mask", "Value"}.issubset(step):
                raise ValueError("incomplete register write")
            records.append((WRITE_ACK, register_write(
                step["Address"], step["Mask"], step["Value"])))
    records.append((WAIT_IRQ, b""))
    if len(records) > MAX_RECORDS:
        raise ValueError("too many touch boot operations")
    payload = bytearray(struct.pack("<4sIII", b"NTFW", 1, len(records), 0))
    for kind, data in records:
        payload.extend(struct.pack("<II", kind, len(data)))
        payload.extend(data)
    struct.pack_into("<I", payload, 12, len(payload))
    return bytes(payload), records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mtprops", type=Path)
    parser.add_argument("calibration_dir", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        if args.output.exists():
            raise ValueError("refusing to overwrite output")
        calibrations = {tag: (args.calibration_dir / f"{tag}.bin").read_bytes()
                        for tag in PROVIDERS.values()}
        stream, records = build_stream(args.mtprops.read_bytes(), calibrations)
        args.output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "wb") as output:
            output.write(stream)
        print(f"D11 touch boot stream: {len(records)} operations, {len(stream)} bytes")
        print(f"SHA-256: {hashlib.sha256(stream).hexdigest()}")
    except (OSError, ValueError, KeyError, TypeError, binascii.Error, ET.ParseError,
            plistlib.InvalidFileException) as error:
        print(f"D11 touch boot stream failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
