#!/usr/bin/env python3
"""Audit Microkit's dynamically allocated UFS backing pool offline.

This proves only that the candidate profile receives one contiguous RAM pool,
maps each page once into its owner, and avoids fixed reserved-memory ranges in
the supplied DTBs. It does not establish an SMMU route or authorize device DMA.
"""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path


PAGE_SIZE = 0x1000
POOL_SIZE = 0x1000000
POOL_PREFIX = "frame_mr_ufs_dma_pool_"
SMMU_TABLE_SIZE = 0xB000
SMMU_TABLE_PREFIX = "frame_mr_ufs_smmu_tables_"


def _fdtget(dtb: Path, *args: str) -> str:
    options = []
    positional = []
    index = 0
    while index < len(args) and args[index].startswith("-"):
        option = args[index]
        options.append(option)
        index += 1
        if option == "-t":
            if index == len(args):
                raise ValueError("missing fdtget type after -t")
            options.append(args[index])
            index += 1
    positional.extend(args[index:])
    return subprocess.check_output(["fdtget", *options, str(dtb), *positional], text=True).strip()


def _cells(dtb: Path, node: str, prop: str) -> list[int]:
    return [int(value, 16) for value in _fdtget(dtb, "-t", "x", node, prop).split()]


def _combine(cells: list[int]) -> int:
    value = 0
    for cell in cells:
        value = (value << 32) | cell
    return value


def _tuples(dtb: Path, cells: list[int], address_cells: int, size_cells: int):
    stride = address_cells + size_cells
    if not cells or len(cells) % stride:
        raise ValueError(f"malformed reg property in {dtb}: {cells}")
    for offset in range(0, len(cells), stride):
        start = _combine(cells[offset : offset + address_cells])
        size = _combine(cells[offset + address_cells : offset + stride])
        if size <= 0:
            raise ValueError(f"empty range in {dtb}: {cells}")
        yield start, size


def read_memory_ranges(dtb: Path) -> list[tuple[int, int]]:
    address_cells = int(_fdtget(dtb, "-t", "x", "/", "#address-cells"), 16)
    size_cells = int(_fdtget(dtb, "-t", "x", "/", "#size-cells"), 16)
    ranges = []
    for node in ["/memory"]:
        props = set(_fdtget(dtb, "-p", node).splitlines())
        if "device_type" not in props or "reg" not in props:
            continue
        if _fdtget(dtb, node, "device_type").rstrip("\0") != "memory":
            continue
        ranges.extend(_tuples(dtb, _cells(dtb, node, "reg"), address_cells, size_cells))
    if not ranges:
        raise ValueError(f"no physical memory ranges in {dtb}")
    return ranges


def read_fixed_reservations(dtb: Path) -> tuple[list[tuple[str, int, int]], int]:
    node = "/reserved-memory"
    address_cells = int(_fdtget(dtb, "-t", "x", node, "#address-cells"), 16)
    size_cells = int(_fdtget(dtb, "-t", "x", node, "#size-cells"), 16)
    fixed: list[tuple[str, int, int]] = []
    dynamic = 0
    for name in _fdtget(dtb, "-l", node).splitlines():
        child = f"{node}/{name}"
        props = set(_fdtget(dtb, "-p", child).splitlines())
        if "reg" in props:
            for start, size in _tuples(dtb, _cells(dtb, child, "reg"), address_cells, size_cells):
                fixed.append((name, start, size))
        elif "size" in props:
            dynamic += 1
    return fixed, dynamic


def _page_tables(spec: dict):
    for index, obj in enumerate(spec["objects"]):
        description = obj.get("object")
        if not isinstance(description, dict):
            continue
        table = description.get("PageTable")
        if table is not None:
            yield index, obj.get("name", ""), table


def _reachable_frames(spec: dict, root: int) -> dict[int, int]:
    objects = spec["objects"]
    counts: dict[int, int] = {}
    visited: set[int] = set()

    def walk(table_id: int) -> None:
        if table_id in visited:
            raise ValueError("page-table cycle or alias")
        visited.add(table_id)
        table = objects[table_id]["object"]["PageTable"]
        for entry in table.get("slots", []):
            cap = entry.get("cap", {})
            frame_id = cap.get("Frame", {}).get("object")
            child_id = cap.get("PageTable", {}).get("object")
            if frame_id is not None:
                counts[frame_id] = counts.get(frame_id, 0) + 1
            if child_id is not None:
                walk(child_id)

    walk(root)
    return counts


def validate_capdl_pool(
    spec: dict,
    expected_size: int = POOL_SIZE,
    owner_pd: str = "modem_image_probe",
) -> tuple[int, int]:
    """Return the allocated [base, size) after checking mapping ownership."""
    objects = spec["objects"]
    expected_pages = expected_size // PAGE_SIZE
    if expected_size <= 0 or expected_size % PAGE_SIZE:
        raise ValueError("pool size must be a positive multiple of 4 KiB")

    frames: dict[int, tuple[int, int]] = {}
    for index, obj in enumerate(objects):
        if not obj.get("name", "").startswith(POOL_PREFIX):
            continue
        description = obj.get("object")
        frame = description.get("Frame") if isinstance(description, dict) else None
        if frame is None or frame.get("paddr") is None or frame.get("size_bits") != 12:
            raise ValueError("UFS pool frame must be a 4 KiB physical frame")
        frames[index] = (int(frame["paddr"]), 1 << frame["size_bits"])
    if len(frames) != expected_pages:
        raise ValueError(f"expected {expected_pages} UFS pool frames, found {len(frames)}")

    physical = sorted(address for address, _ in frames.values())
    base = physical[0]
    if physical != list(range(base, base + expected_size, PAGE_SIZE)):
        raise ValueError("UFS pool frames must form one contiguous physical interval")

    root_name = f"pgd_{owner_pd}"
    roots = [index for index, name, table in _page_tables(spec)
             if name == root_name and table.get("is_root")]
    if len(roots) != 1:
        raise ValueError(f"expected one {owner_pd} root page table")
    root_counts = _reachable_frames(spec, roots[0])
    if any(root_counts.get(index) != 1 for index in frames):
        raise ValueError("every UFS pool page must be mapped exactly once in the owner")

    global_counts: dict[int, int] = {}
    for _, _, table in _page_tables(spec):
        for entry in table.get("slots", []):
            frame_id = entry.get("cap", {}).get("Frame", {}).get("object")
            if frame_id in frames:
                global_counts[frame_id] = global_counts.get(frame_id, 0) + 1
    if any(global_counts.get(index) != 1 for index in frames):
        raise ValueError("UFS pool pages must have no extra page-table aliases")

    return base, expected_size


def validate_dtb_range(dtb: Path, base: int, size: int) -> tuple[int, int]:
    end = base + size
    memory = read_memory_ranges(dtb)
    if not any(base >= start and end <= start + length for start, length in memory):
        raise ValueError(f"UFS pool {base:#x}..{end:#x} is outside RAM described by {dtb}")
    reservations, dynamic_count = read_fixed_reservations(dtb)
    overlaps = [name for name, start, length in reservations
                if base < start + length and start < end]
    if overlaps:
        raise ValueError(f"UFS pool overlaps fixed reserved-memory in {dtb}: {overlaps}")
    return len(reservations), dynamic_count


def validate_smmu_table_frames(
    spec: dict,
    pool_base: int,
    pool_size: int,
    owner_pd: str = "modem_image_probe",
) -> tuple[int, int]:
    """Check the separate contiguous CPU-mapped LPAE table-page allocation."""
    objects = spec["objects"]
    expected_pages = SMMU_TABLE_SIZE // PAGE_SIZE
    frames: dict[int, int] = {}
    for index, obj in enumerate(objects):
        if not obj.get("name", "").startswith(SMMU_TABLE_PREFIX):
            continue
        description = obj.get("object")
        frame = description.get("Frame") if isinstance(description, dict) else None
        if frame is None or frame.get("paddr") is None or frame.get("size_bits") != 12:
            raise ValueError("UFS SMMU table must use 4 KiB physical frames")
        frames[index] = int(frame["paddr"])
    if len(frames) != expected_pages:
        raise ValueError(f"expected {expected_pages} UFS SMMU table frames, found {len(frames)}")

    physical = sorted(frames.values())
    base = physical[0]
    if physical != list(range(base, base + SMMU_TABLE_SIZE, PAGE_SIZE)):
        raise ValueError("UFS SMMU table frames must form one contiguous physical interval")
    if base < pool_base + pool_size and pool_base < base + SMMU_TABLE_SIZE:
        raise ValueError("UFS SMMU table frames must be separate from DMA-visible pool pages")

    roots = [
        index for index, name, table in _page_tables(spec)
        if name == f"pgd_{owner_pd}" and table.get("is_root")
    ]
    if len(roots) != 1:
        raise ValueError(f"expected one {owner_pd} root page table")
    root_counts = _reachable_frames(spec, roots[0])
    if any(root_counts.get(index) != 1 for index in frames):
        raise ValueError("every UFS SMMU table page must be mapped exactly once in the owner")

    global_counts: dict[int, int] = {}
    for _, _, table in _page_tables(spec):
        for entry in table.get("slots", []):
            frame_id = entry.get("cap", {}).get("Frame", {}).get("object")
            if frame_id in frames:
                global_counts[frame_id] = global_counts.get(frame_id, 0) + 1
    if any(global_counts.get(index) != 1 for index in frames):
        raise ValueError("UFS SMMU table pages must have no extra page-table aliases")
    return base, SMMU_TABLE_SIZE


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capdl", required=True, type=Path)
    parser.add_argument("--dtb", required=True, action="append", type=Path)
    parser.add_argument("--owner-pd", default="modem_image_probe")
    parser.add_argument("--smmu-tables", action="store_true",
                        help="also audit the separate eleven-page LPAE table region")
    args = parser.parse_args()
    base, size = validate_capdl_pool(
        json.loads(args.capdl.read_text()), owner_pd=args.owner_pd
    )
    print(f"UFS backing pool: {base:#x}..{base + size:#x} ({size:#x} bytes), contiguous and singly mapped")
    tables = validate_smmu_table_frames(
        json.loads(args.capdl.read_text()), base, size, owner_pd=args.owner_pd
    ) if args.smmu_tables else None
    if tables is not None:
        print(f"UFS SMMU tables: {tables[0]:#x}..{tables[0] + tables[1]:#x} ({tables[1]:#x} bytes), separate and singly mapped")
    for dtb in args.dtb:
        fixed_count, dynamic_count = validate_dtb_range(dtb, base, size)
        print(f"{dtb}: within RAM; no overlap with {fixed_count} fixed reservations; "
              f"{dynamic_count} dynamic reservations have no address to compare")
        if tables is not None:
            validate_dtb_range(dtb, *tables)
    print("Backing RAM only: no SMMU route, DMA transaction, UFS start, or secure ownership is implied.")


if __name__ == "__main__":
    main()
