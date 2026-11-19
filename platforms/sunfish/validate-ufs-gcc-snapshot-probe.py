#!/usr/bin/env python3
"""Audit the temporary UFS GCC snapshot's exact read-only authority."""

from __future__ import annotations

import argparse
import json
import xml.etree.ElementTree as ET
from pathlib import Path


EXPECTED_REGIONS = {
    "gcc_ufs_local": ("0x00177000", "0x1000", "0x1000"),
    "gcc_ufs_aggregate": ("0x00182000", "0x1000", "0x1000"),
    "gcc_ufs_mem_ref": ("0x0018c000", "0x1000", "0x1000"),
    "splash": ("0x9c000000", "0x1600000", "0x200000"),
}
GCC_PHYSICAL = {
    "gcc_ufs_local": 0x00177000,
    "gcc_ufs_aggregate": 0x00182000,
    "gcc_ufs_mem_ref": 0x0018C000,
}
EXPECTED_MAPS = {
    "gcc_ufs_local": ("0x64000000", "r", "false", "gcc_ufs_local"),
    "gcc_ufs_aggregate": ("0x64001000", "r", "false", "gcc_ufs_aggregate"),
    "gcc_ufs_mem_ref": ("0x64002000", "r", "false", "gcc_ufs_mem_ref"),
    "splash": ("0x42000000", "rw", "false", "splash_fb"),
}
EXPECTED_READS = (
    'checkpoint_read(&display, local, 0x004, "GDSCR")',
    'checkpoint_read(&display, local, 0x000, "PHY BCR")',
    'checkpoint_read(&display, local, 0x020, "AXI CMD")',
    'checkpoint_read(&display, local, 0x024, "AXI CFG")',
    'checkpoint_read(&display, local, 0x060, "UNIPRO CMD")',
    'checkpoint_read(&display, local, 0x064, "UNIPRO CFG")',
    'checkpoint_read(&display, local, 0x048, "ICE CMD")',
    'checkpoint_read(&display, local, 0x04c, "ICE CFG")',
    'checkpoint_read(&display, local, 0x098, "AUX CMD")',
    'checkpoint_read(&display, local, 0x09c, "AUX CFG")',
    'checkpoint_read(&display, local, 0x014, "AHB CBCR")',
    'checkpoint_read(&display, local, 0x018, "TX SYM CBCR")',
    'checkpoint_read(&display, local, 0x01c, "RX SYM CBCR")',
    'checkpoint_read(&display, local, 0x038, "AXI CBCR")',
    'checkpoint_read(&display, local, 0x08c, "UNIPRO CBCR")',
    'checkpoint_read(&display, local, 0x090, "ICE CBCR")',
    'checkpoint_read(&display, local, 0x094, "AUX CBCR")',
    'checkpoint_read(&display, aggregate, 0x024, "AGGR AXI CBCR")',
    'checkpoint_read(&display, mem_ref, 0x000, "MEM REF CBCR")',
)
EXPECTED_MAP_CHECKPOINTS = (
    (
        'display.replace_line(1, "MAP LOCAL")',
        "var!(gcc_ufs_local: usize = 0)",
        "0x6400_0000",
        '"LOCAL MAPPED"',
    ),
    (
        'display.replace_line(1, "MAP AGGREGATE")',
        "var!(gcc_ufs_aggregate: usize = 0)",
        "0x6400_1000",
        '"AGG MAPPED"',
    ),
    (
        'display.replace_line(1, "MAP MEM REF")',
        "var!(gcc_ufs_mem_ref: usize = 0)",
        "0x6400_2000",
        '"MEM REF MAPPED"',
    ),
)


def validate(system_path: Path, source_path: Path, decoder_path: Path) -> None:
    root = ET.parse(system_path).getroot()
    regions_list = root.findall("memory_region")
    regions = {region.get("name"): region for region in regions_list}
    assert len(regions) == len(regions_list), "duplicate memory region"
    assert set(regions) == set(EXPECTED_REGIONS), "unexpected mapped physical region"
    for name, (physical, size, page_size) in EXPECTED_REGIONS.items():
        assert regions[name].attrib == {
            "name": name,
            "phys_addr": physical,
            "size": size,
            "page_size": page_size,
        }, f"{name} physical window changed"

    domains = root.findall("protection_domain")
    assert len(domains) == 1, "the probe must have one protection domain"
    domain = domains[0]
    assert domain.get("name") == "ufs_gcc_snapshot_probe"
    assert domain.get("smc") == "false", "the probe must have no SMC capability"
    assert domain.find("program_image").get("path") == "ufs_gcc_snapshot_probe.elf"
    maps_list = domain.findall("map")
    maps = {mapping.get("mr"): mapping for mapping in maps_list}
    assert len(maps) == len(maps_list), "duplicate memory mapping"
    assert set(maps) == set(EXPECTED_MAPS), "unexpected mapping authority"
    for name, (vaddr, perms, cached, symbol) in EXPECTED_MAPS.items():
        expected = {"mr": name, "vaddr": vaddr, "perms": perms, "cached": cached}
        expected["setvar_vaddr"] = symbol
        assert maps[name].attrib == expected, f"{name} mapping permissions changed"
    assert not domain.findall("irq") and not root.findall("channel"), "no IRQ or channel"
    assert not domain.findall("setvar"), "no device physical-address or DMA authority"

    source = source_path.read_text()
    decoder = decoder_path.read_text()
    assert "read_volatile()" in source, "the probe must use volatile read-only MMIO"
    assert "write_volatile" not in source, "the probe source must not write MMIO"
    assert "REG_CAPABILITIES" not in source and "HCI MMIO" in source
    assert 'mod qcom_ufs_gcc;' in source, "the probe must use the tested GCC-state decoder"
    assert "check_gcc_pre_cap_state()" in source, "the probe must classify the raw snapshot"
    assert "pub const RCG_ROOT_OFF: u32 = 1 << 31;" in decoder
    root_check = decoder.split("fn root_is_ready(", 1)[1].split(
        "fn gate_is_ready(", 1
    )[0]
    assert "command & RCG_ROOT_ENABLE_REQUEST == 0" not in root_check
    assert "command & RCG_ROOT_OFF != 0" in root_check
    assert "self.hid_divider == 1 && observed & RCG_CFG_DIVIDER_MASK == 0" in decoder
    assert "memory_region_symbol!(gcc_ufs_local" not in source
    assert "address == expected" in source, "mapping address must be checked before MMIO"
    assert 'display.replace_line(3, if valid { completed } else { "ADDR INVALID" })' in source
    map_positions = []
    cursor = 0
    for marker, symbol, expected, completed in EXPECTED_MAP_CHECKPOINTS:
        position = source.find(marker, cursor)
        assert position >= 0, f"missing pre-map checkpoint: {marker}"
        symbol_position = source.find(symbol, position)
        assert symbol_position >= 0, f"missing symbol lookup after checkpoint: {symbol}"
        expected_position = source.find(expected, symbol_position)
        assert expected_position >= 0, f"missing expected virtual address: {expected}"
        completed_position = source.find(completed, symbol_position)
        assert completed_position >= 0, f"missing completed-map checkpoint: {completed}"
        map_positions.extend((position, symbol_position, expected_position, completed_position))
        cursor = completed_position + len(completed)
    first_read_position = source.find(EXPECTED_READS[0])
    assert first_read_position > map_positions[-1], "GCC reads must follow all map checkpoints"
    for read in EXPECTED_READS:
        assert read in source, f"missing source-expected GCC read checkpoint: {read}"
    checkpoint = source.split("fn checkpoint_read(", 1)[1].split(
        "fn readiness_label(", 1
    )[0]
    checkpoint_order = (
        'display.replace_line(1, "READING")',
        'display.hex(2, "OFFSET", offset)',
        "display.replace_line(3, label)",
        "display.flush()",
        "let value = read_register(base, offset)",
        'display.replace_line(1, "READ OK")',
        "display.hex(2, label, value)",
        "display.flush()",
    )
    positions = []
    cursor = 0
    for needle in checkpoint_order:
        position = checkpoint.find(needle, cursor)
        positions.append(position)
        if position >= 0:
            cursor = position + len(needle)
    assert all(position >= 0 for position in positions), "missing per-read progress display"
    assert positions == sorted(positions), "per-read progress is not shown around MMIO access"


def validate_capdl(capdl_path: Path) -> None:
    capdl = json.loads(capdl_path.read_text())
    objects = capdl["objects"]
    frame_indices: dict[str, int] = {}
    for index, item in enumerate(objects):
        capdl_object = item.get("object")
        if not isinstance(capdl_object, dict):
            continue
        frame = capdl_object.get("Frame")
        if frame is None:
            continue
        for name, physical in GCC_PHYSICAL.items():
            if frame.get("paddr") == physical:
                assert name not in frame_indices, f"duplicate CapDL frame for {name}"
                frame_indices[name] = index
                assert frame.get("size_bits") == 12, f"{name} must stay one 4 KiB page"
    assert set(frame_indices) == set(GCC_PHYSICAL), "CapDL GCC frame set changed"

    mapped: dict[str, list[dict]] = {name: [] for name in GCC_PHYSICAL}
    for item in objects:
        capdl_object = item.get("object")
        if not isinstance(capdl_object, dict):
            continue
        page_table = capdl_object.get("PageTable")
        if page_table is None:
            continue
        for slot in page_table.get("slots", []):
            frame_cap = slot.get("cap", {}).get("Frame")
            if frame_cap is None:
                continue
            for name, frame_index in frame_indices.items():
                if frame_cap.get("object") == frame_index:
                    mapped[name].append(frame_cap)
    for name, caps in mapped.items():
        assert len(caps) == 1, f"{name} must be mapped exactly once"
        cap = caps[0]
        assert cap.get("rights") == {
            "read": True,
            "write": False,
            "grant": False,
            "grant_reply": False,
        }, f"{name} CapDL mapping is not read-only"
        assert cap.get("cached") is False, f"{name} must remain uncached device memory"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--system", required=True, type=Path)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--decoder", required=True, type=Path)
    parser.add_argument("--capdl-json", type=Path)
    args = parser.parse_args()
    validate(args.system, args.source, args.decoder)
    if args.capdl_json is not None:
        validate_capdl(args.capdl_json)
    print("UFS GCC probe authority passed: three read-only GCC pages + splash; no HCI, IRQ, SMC or DMA")


if __name__ == "__main__":
    main()
