#!/usr/bin/env python3
"""Add one asynchronous, view-only scanning PD per secondary vCPU to a UTM image."""
import argparse
import xml.etree.ElementTree as ET
from pathlib import Path


def configure(tree, cpus):
    root = tree.getroot()
    wallet = root.find("protection_domain[@name='wallet_app']")
    if wallet is None or wallet.get("cpu", "0") != "0":
        raise ValueError("expected the wallet on CPU zero")
    for cpu in range(1, cpus):
        name = f"wallet_scan_{cpu}"
        slot = f"scan_mailbox_{cpu}"
        heap = f"{name}_heap"
        channel = 15 + cpu
        for tag, obj in (("protection_domain", name), ("memory_region", slot), ("memory_region", heap)):
            if root.find(f"{tag}[@name='{obj}']") is not None:
                raise ValueError(f"duplicate {obj}")
        if any(int(e.get("id"), 0) == channel for e in root.findall(".//end[@pd='wallet_app']")):
            raise ValueError(f"wallet channel {channel} already in use")
        for region, size in ((slot, "0x400000"), (heap, "0x1000000")):
            ET.SubElement(root, "memory_region", name=region, size=size, page_size="0x200000")
        worker = ET.SubElement(root, "protection_domain", name=name, cpu=str(cpu), priority="100",
                               budget="10000", period="10000", stack_size="0x20000")
        ET.SubElement(worker, "program_image", path="rust_wallet_range_worker.elf")
        ET.SubElement(worker, "map", mr=slot, vaddr="0x50000000", perms="rw", setvar_vaddr="range_mailbox")
        ET.SubElement(worker, "map", mr=heap, vaddr="0x52000000", perms="rw", setvar_vaddr="range_heap")
        ET.SubElement(wallet, "map", mr=slot, vaddr=hex(0x5a000000 + 0x400000 * (cpu-1)),
                      perms="rw", setvar_vaddr=slot)
        link = ET.SubElement(root, "channel")
        ET.SubElement(link, "end", pd="wallet_app", id=str(channel), notify="true")
        ET.SubElement(link, "end", pd=name, id="0", notify="false")
    ET.indent(tree, space="    ")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--cpus", type=int, choices=(1, 2, 4), default=4)
    args = parser.parse_args()
    tree = ET.parse(args.source)
    configure(tree, args.cpus)
    tree.write(args.output, encoding="utf-8", xml_declaration=True)
