#!/usr/bin/env python3
"""Add the generated sDDF virtio block components to the regular system."""
import argparse
from pathlib import Path
import xml.etree.ElementTree as ET


BLOCK_MR_PREFIXES = (
    "virtio_mmio@a000200/drivers/blk/virtio/",
    "blk_driver_",
    "blk_client_vault_",
)
def parse(path):
    parser = ET.XMLParser(target=ET.TreeBuilder(insert_comments=True))
    return ET.parse(path, parser=parser)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--storage", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    base_tree = parse(args.base)
    storage_root = parse(args.storage).getroot()
    root = base_tree.getroot()

    base_vault = root.find("./protection_domain[@name='vault']")
    storage_vault = storage_root.find("./protection_domain[@name='vault']")
    if base_vault is None or storage_vault is None:
        raise RuntimeError("both systems must define a top-level vault protection domain")
    image = base_vault.find("program_image")
    if image is None or image.get("path") != "rust_vault_disk.elf":
        raise RuntimeError("base vault must use the disk-backed Rust image")

    memory_regions = {node.get("name") for node in root.findall("memory_region")}
    for region in storage_root.findall("memory_region"):
        name = region.get("name", "")
        if name.startswith(BLOCK_MR_PREFIXES):
            if name in memory_regions:
                raise RuntimeError(f"duplicate memory region while merging: {name}")
            root.append(region)
            memory_regions.add(name)

    for mapping in storage_vault.findall("map"):
        name = mapping.get("mr", "")
        if name.startswith("blk_client_vault_"):
            if any(existing.get("mr") == name for existing in base_vault.findall("map")):
                raise RuntimeError(f"duplicate vault map while merging: {name}")
            base_vault.append(mapping)

    for name in ("blk_driver", "blk_virt"):
        if root.find(f"./protection_domain[@name='{name}']") is not None:
            raise RuntimeError(f"base system already contains {name}")
        component = storage_root.find(f"./protection_domain[@name='{name}']")
        if component is None:
            raise RuntimeError(f"storage system is missing {name}")
        root.append(component)

    # The block virtualiser uses vault endpoint 0; owner and child calls use 1 and 2.
    required_channels = {
        frozenset({("vault", "1"), ("client", "6")}),
        frozenset({("vault", "2"), ("crasher", "1")}),
    }
    present_channels = {
        frozenset((end.get("pd"), end.get("id")) for end in channel.findall("end"))
        for channel in root.findall("channel")
    }
    if not required_channels <= present_channels:
        raise RuntimeError("base system has unexpected vault owner or crasher channel IDs")

    for channel in storage_root.findall("channel"):
        ends = channel.findall("end")
        names = {end.get("pd") for end in ends}
        if names <= {"blk_driver", "blk_virt", "vault"} and names & {"blk_driver", "blk_virt"}:
            root.append(channel)

    # Reject dangling references and duplicate endpoint IDs before Microkit runs.
    pd_names = {pd.get("name") for pd in root.findall(".//protection_domain")}
    for mapping in root.findall(".//map"):
        if mapping.get("mr") not in memory_regions:
            raise RuntimeError(f"mapping references missing memory region: {mapping.get('mr')}")
    endpoint_ids = set()
    for channel in root.findall("channel"):
        for end in channel.findall("end"):
            if end.get("pd") not in pd_names:
                raise RuntimeError(f"channel references missing protection domain: {end.get('pd')}")
            endpoint = (end.get("pd"), end.get("id"))
            if endpoint in endpoint_ids:
                raise RuntimeError(f"protection-domain channel endpoint is reused: {endpoint}")
            endpoint_ids.add(endpoint)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    base_tree.write(args.output, encoding="UTF-8", xml_declaration=True)


if __name__ == "__main__":
    main()
