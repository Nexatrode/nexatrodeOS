#!/usr/bin/env python3
"""Re-encrypt authenticated nexatrodeOS vault records in a copy of a stopped VM disk."""

import argparse
import os
from pathlib import Path
import re
import struct

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCMSIV


SECTOR_SIZE = 512
BLOCK_SIZE = 4096


def development_key():
    source = Path(__file__).resolve().parents[1] / "rust/src/vault_crypto.rs"
    match = re.search(r"const DEVELOPMENT_KEY:.*?= \[(.*?)\];", source.read_text(), re.S)
    key = bytes(int(value, 16) for value in re.findall(r"0x[0-9a-f]{2}", match[1]))
    if len(key) != 32:
        raise ValueError("invalid development key definition")
    return key


def vault_blocks(image):
    header = SECTOR_SIZE
    if image[header:header + 8] != b"EFI PART":
        raise ValueError("expected a GPT disk image with 512-byte sectors")
    entries, count, size = struct.unpack_from("<QII", image, header + 72)
    if size < 128 or entries * SECTOR_SIZE + count * size > len(image):
        raise ValueError("invalid GPT partition table")
    partitions = []
    for i in range(count):
        offset = entries * SECTOR_SIZE + i * size
        if any(image[offset:offset + 16]):
            first, last = struct.unpack_from("<QQ", image, offset + 32)
            partitions.append((first * SECTOR_SIZE, (last + 1) * SECTOR_SIZE))
    if len(partitions) != 1:
        raise ValueError("expected one vault partition")
    first, end = partitions[0]
    if first < 2 * SECTOR_SIZE or end > len(image) or end - first < BLOCK_SIZE:
        raise ValueError("invalid vault partition extent")
    return range(first, end - BLOCK_SIZE + 1, BLOCK_SIZE)


def rekey_record(block, old, new):
    """Rekey authenticated records while preserving their commit markers."""
    magic = block[:8]
    if magic not in (b"PVLTREC1", b"PVLTBLB1"):
        return False

    version, generation = struct.unpack_from("<IQ", block, 8)
    if version != 1:
        raise ValueError(f"unsupported vault record version {version}")
    if magic == b"PVLTREC1":
        nonce_offset, start, length = 20, 32, 8
        nonce_magic, aad_magic, salt = b"PV1", b"PVLTAD01", 0x6A09E667F3BCC909
    else:
        length, = struct.unpack_from("<I", block, 20)
        nonce_offset, start = 24, 36
        nonce_magic, aad_magic, salt = b"PB1", b"PVLTBL1A", 0x3C6EF372FE94F82B
        if length > BLOCK_SIZE - start - 24:
            raise ValueError("invalid vault blob length")

    tag_end = start + length + 16
    marker, = struct.unpack_from("<Q", block, tag_end)
    committed = marker == generation ^ salt
    nonce = block[nonce_offset:start]
    namespace = nonce[3]
    if nonce != nonce_magic + bytes([namespace]) + struct.pack("<Q", generation):
        if committed:
            raise ValueError("invalid vault record nonce")
        return False
    aad = aad_magic + struct.pack("<IQQ", version, namespace, generation)
    if magic == b"PVLTBLB1":
        aad += struct.pack("<I", length)
    try:
        plaintext = old.decrypt(nonce, block[start:tag_end], aad)
    except InvalidTag:
        if committed:
            raise
        return False
    block[start:tag_end] = new.encrypt(nonce, plaintext, aad)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("key_file", type=Path, help="private file containing 32 random bytes")
    args = parser.parse_args()
    old_key = development_key()
    new_key = args.key_file.read_bytes()
    if len(new_key) != 32 or not any(new_key) or new_key == old_key:
        parser.error("the installation key must be 32 new random bytes")
    old, new = AESGCMSIV(old_key), AESGCMSIV(new_key)
    image = bytearray(args.source.read_bytes())
    count = 0
    for offset in vault_blocks(image):
        block = image[offset:offset + BLOCK_SIZE]
        try:
            if rekey_record(block, old, new):
                image[offset:offset + BLOCK_SIZE] = block
                count += 1
        except (ValueError, InvalidTag) as error:
            raise ValueError(f"could not migrate vault block at disk offset {offset}: {error}") from error
    if count == 0:
        raise ValueError("no authenticated development-key vault records were found")
    descriptor = os.open(args.destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(image)
            output.flush()
            os.fsync(output.fileno())
    except BaseException:
        args.destination.unlink()
        raise
    print(f"Migrated {count} authenticated vault records.")


if __name__ == "__main__":
    main()
