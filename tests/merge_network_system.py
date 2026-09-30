#!/usr/bin/env python3
"""Merge the flat tests/net VirtIO Ethernet fragment into Personal OS.

VirtIO MMIO devices on bus.0 (net) and bus.1 (blk) share the same 4 KiB page at
0xa000000, so the ethernet driver reuses the block driver's regs region. DMA
buffers that sdfgen places at the top of RAM are relocated below Personal OS
storage/RNG reservations, and matching phys addresses in the net ELFs are
patched.
"""
import argparse
import struct
from pathlib import Path
import xml.etree.ElementTree as ET


PAGE = 0x1000
# Carve net DMA just below the lowest Personal OS DMA reservation on QEMU virt.
DMA_TOP = 0x5FDD_C000
DMA_FLOOR = 0x4000_0000


def parse(path):
    parser = ET.XMLParser(target=ET.TreeBuilder(insert_comments=True))
    return ET.parse(path, parser=parser)


def region_span(node):
    phys = node.get("phys_addr")
    if not phys:
        return None
    start = int(phys, 0)
    size = int(node.get("size"), 0)
    return start, start + size


def overlaps(a0, a1, b0, b1):
    return a0 < b1 and b0 < a1


def occupied_ranges(root):
    ranges = []
    for region in root.findall("memory_region"):
        span = region_span(region)
        if span is not None:
            ranges.append(span)
    return ranges


def allocate(size, occupied):
    size = (size + PAGE - 1) & ~(PAGE - 1)
    candidate = (DMA_TOP - size) & ~(PAGE - 1)
    while candidate >= DMA_FLOOR:
        end = candidate + size
        if not any(overlaps(candidate, end, start, stop) for start, stop in occupied):
            occupied.append((candidate, end))
            return candidate
        candidate -= PAGE
    raise RuntimeError(f"unable to allocate {size:#x} bytes of DMA below {DMA_TOP:#x}")


def find_shareable(root, start, end):
    """Return a base MR that covers the same page-aligned MMIO span."""
    for region in root.findall("memory_region"):
        span = region_span(region)
        if span is None:
            continue
        base_start, base_end = span
        if base_start == start and base_end == end:
            return region.get("name")
        # Allow sharing when both describe the same page-aligned window.
        if base_start <= start and end <= base_end and (end - start) == PAGE:
            return region.get("name")
    return None


NET_ELF_NAMES = (
    "eth_driver.elf",
    "dhcp_client.elf",
    "network_virt_rx.elf",
    "network_virt_tx.elf",
    "network_copy.elf",
    "timer_driver.elf",
)


def patch_phys_in_elfs(elf_dir, phys_remap):
    if not phys_remap:
        return
    replacements = [
        (struct.pack("<Q", old), struct.pack("<Q", new))
        for old, new in phys_remap.items()
        if old != new
    ]
    for name in NET_ELF_NAMES:
        path = elf_dir / name
        if not path.exists():
            continue
        data = bytearray(path.read_bytes())
        changed = False
        for old_bytes, new_bytes in replacements:
            start = 0
            while True:
                index = data.find(old_bytes, start)
                if index < 0:
                    break
                data[index:index + 8] = new_bytes
                changed = True
                start = index + 8
        if changed:
            path.write_bytes(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--network", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--elf-dir",
        type=Path,
        help="Directory of net ELFs whose embedded DMA phys addresses should be patched",
    )
    args = parser.parse_args()

    base_tree = parse(args.base)
    network_root = parse(args.network).getroot()
    root = base_tree.getroot()

    memory_regions = {node.get("name") for node in root.findall("memory_region")}
    occupied = occupied_ranges(root)
    name_remap = {}
    phys_remap = {}

    for region in network_root.findall("memory_region"):
        name = region.get("name", "")
        if name in memory_regions:
            raise RuntimeError(f"duplicate memory region while merging network: {name}")

        span = region_span(region)
        if span is None:
            root.append(region)
            memory_regions.add(name)
            continue

        start, end = span
        collision = next(
            (
                (other_start, other_end)
                for other_start, other_end in occupied
                if overlaps(start, end, other_start, other_end)
            ),
            None,
        )
        if collision is None:
            root.append(region)
            memory_regions.add(name)
            occupied.append((start, end))
            continue

        shared = find_shareable(root, start, end)
        if shared is not None and (end - start) <= PAGE:
            # Net and blk VirtIO MMIO regs share page 0xa000000.
            name_remap[name] = shared
            continue

        new_phys = allocate(end - start, occupied)
        phys_remap[start] = new_phys
        region.set("phys_addr", f"{new_phys:#x}")
        root.append(region)
        memory_regions.add(name)

    pd_names = {pd.get("name") for pd in root.findall(".//protection_domain")}
    for component in network_root.findall("protection_domain"):
        name = component.get("name", "")
        if name in pd_names:
            raise RuntimeError(f"duplicate protection domain while merging network: {name}")
        for mapping in component.findall("map"):
            mr = mapping.get("mr")
            if mr in name_remap:
                mapping.set("mr", name_remap[mr])
        root.append(component)
        pd_names.add(name)

    for channel in network_root.findall("channel"):
        root.append(channel)

    for mapping in root.findall(".//map"):
        if mapping.get("mr") not in memory_regions:
            raise RuntimeError(
                f"mapping references missing memory region: {mapping.get('mr')}"
            )
    endpoint_ids = set()
    for channel in root.findall("channel"):
        for endpoint in channel.findall("end"):
            if endpoint.get("pd") not in pd_names:
                raise RuntimeError(
                    f"channel references missing protection domain: {endpoint.get('pd')}"
                )
            endpoint_key = (endpoint.get("pd"), endpoint.get("id"))
            if endpoint_key in endpoint_ids:
                raise RuntimeError(
                    f"protection-domain channel endpoint is reused: {endpoint_key}"
                )
            endpoint_ids.add(endpoint_key)

    if args.elf_dir is not None:
        patch_phys_in_elfs(args.elf_dir, phys_remap)

    inject_messages_net_bridge(root, memory_regions, pd_names, endpoint_ids)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    base_tree.write(args.output, encoding="UTF-8", xml_declaration=True)


def inject_messages_net_bridge(root, memory_regions, pd_names, endpoint_ids):
    """Map messages_net_tx into dhcp_client and add Messages→dhcp pp channel."""
    if "messages_app" not in pd_names or "dhcp_client" not in pd_names:
        raise RuntimeError("messages_app and dhcp_client required for sealed TX bridge")
    if "messages_net_tx" not in memory_regions:
        raise RuntimeError("base system missing messages_net_tx memory region")

    dhcp = next(
        pd for pd in root.findall(".//protection_domain") if pd.get("name") == "dhcp_client"
    )
    # Protected calls require the callee to outrank the caller (messages_app=100).
    dhcp.set("priority", "101")
    if not any(mapping.get("mr") == "messages_net_tx" for mapping in dhcp.findall("map")):
        mapping = ET.SubElement(dhcp, "map")
        mapping.set("mr", "messages_net_tx")
        mapping.set("vaddr", "0x49_000_000")
        mapping.set("perms", "rw")
        mapping.set("setvar_vaddr", "messages_net_tx")

    for endpoint_key in (("messages_app", "3"), ("dhcp_client", "3")):
        if endpoint_key in endpoint_ids:
            raise RuntimeError(f"channel endpoint already used: {endpoint_key}")

    channel = ET.SubElement(root, "channel")
    messages_end = ET.SubElement(channel, "end")
    messages_end.set("pd", "messages_app")
    messages_end.set("id", "3")
    messages_end.set("pp", "true")
    messages_end.set("notify", "false")
    dhcp_end = ET.SubElement(channel, "end")
    dhcp_end.set("pd", "dhcp_client")
    dhcp_end.set("id", "3")
    dhcp_end.set("notify", "false")
    endpoint_ids.add(("messages_app", "3"))
    endpoint_ids.add(("dhcp_client", "3"))


if __name__ == "__main__":
    main()
