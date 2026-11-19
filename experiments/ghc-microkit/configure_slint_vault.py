#!/usr/bin/env python3
"""Align an sDDF block-client config with the vault maps in a system file."""

import argparse
import struct
import xml.etree.ElementTree as ET
from pathlib import Path


def remap_config(config: bytes, system: str) -> bytes:
    # BlockClientConfig: magic/padding, three Region pairs, connection fields,
    # then the data Region. Preserve queue capacity, ID, sizes and padding.
    if len(config) != 80 or config[:5] != b"sDDF\x02":
        raise ValueError("unexpected sDDF block-client configuration")
    root = ET.fromstring(system)
    vaults = [pd for pd in root.iter("protection_domain") if pd.get("name") == "vault"]
    if len(vaults) != 1:
        raise ValueError("expected one vault protection domain")
    regions = {}
    for region in root.iter("memory_region"):
        name = region.get("name")
        if not name or name in regions:
            raise ValueError("missing or duplicate memory region name")
        regions[name] = int(region.attrib["size"].replace("_", ""), 0)
    config = bytearray(config)
    all_maps = vaults[0].findall("map")
    for name, offset, perms in (
        ("blk_client_vault_storage_info", 8, "r"),
        ("blk_client_vault_request", 24, "rw"),
        ("blk_client_vault_response", 40, "rw"),
        ("blk_client_vault_data", 64, "rw"),
    ):
        maps = [entry for entry in all_maps if entry.get("mr") == name]
        if len(maps) != 1 or name not in regions:
            raise ValueError(f"missing or duplicate vault map: {name}")
        address = int(maps[0].attrib["vaddr"].replace("_", ""), 0)
        length = struct.unpack_from("<Q", config, offset + 8)[0]
        if not address or address % 4096 or length != regions[name] or maps[0].get("perms") != perms:
            raise ValueError(f"invalid vault mapping or size: {name}")
        for other in all_maps:
            if other is maps[0]:
                continue
            other_name = other.get("mr")
            if other_name not in regions:
                raise ValueError(f"unknown mapped region: {other_name}")
            other_address = int(other.attrib["vaddr"].replace("_", ""), 0)
            if address < other_address + regions[other_name] and other_address < address + length:
                raise ValueError(f"overlapping vault mappings: {name} and {other_name}")
        struct.pack_into("<Q", config, offset, address)
    return bytes(config)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("system", type=Path)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.write_bytes(remap_config(args.input.read_bytes(), args.system.read_text()))


if __name__ == "__main__":
    main()
