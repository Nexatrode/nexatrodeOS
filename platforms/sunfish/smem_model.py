"""Bounded, offline parser for Qualcomm SMEM v12 private-item fixtures.

This module consumes caller-provided bytes only. It has no MMIO, device,
hwspinlock, or shared-memory mapping code and does not establish that live
SMEM may safely be inspected or that a caller may use a resolved item.
"""

from __future__ import annotations

from dataclasses import dataclass
import struct


SMEM_PTABLE_MAGIC = b"$TOC"
SMEM_PART_MAGIC = b"$PRT"
SMEM_PRIVATE_CANARY = 0xA5A5
SMEM_HOST_APPS = 0
SMEM_HOST_MODEM = 1
SMEM_GLOBAL_PART_VERSION = 12
SMEM_MASTER_SBL_VERSION_INDEX = 7
SMEM_ITEM_COUNT = 512
SMEM_HEADER_SIZE = 8400
SMEM_PTABLE_HEADER_SIZE = 32
SMEM_PTABLE_ENTRY_SIZE = 48
SMEM_PARTITION_HEADER_SIZE = 32
SMEM_PRIVATE_ENTRY_SIZE = 16
SMEM_PTABLE_BYTES = 4096


class SmemFormatError(ValueError):
    """Malformed, unsupported, or out-of-bounds SMEM data."""


@dataclass(frozen=True)
class SmemItem:
    item_id: int
    offset: int
    size: int
    host0: int
    host1: int


def _u16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]


def _u32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def _range_end(offset: int, size: int, limit: int, label: str) -> int:
    end = offset + size
    if offset < 0 or size < 0 or end < offset or end > limit:
        raise SmemFormatError(f"{label} is outside the supplied SMEM region")
    return end


def resolve_private_uncached_item(
    region: bytes,
    item_id: int,
    *,
    local_host: int = SMEM_HOST_APPS,
    remote_host: int = SMEM_HOST_MODEM,
    include_cached: bool = False,
) -> SmemItem:
    """Resolve one uncached item from a host-pair private partition.

    Mirrors the relevant bounds and canary checks of matched Linux
    qcom_smem_get() for an SMEM v12 partition table. Cached allocations are
    intentionally unsupported and fail as not found; live metadata requires
    the platform hwspinlock and safe mapping contract before this parser could
    be applied to a device snapshot.
    """
    if len(region) < SMEM_HEADER_SIZE + SMEM_PTABLE_BYTES:
        raise SmemFormatError("SMEM region is too small for header and partition table")
    if _u32(region, 192) != 1 or _u32(region, 204) != 0:
        raise SmemFormatError("SMEM header is not initialized or has reserved bits set")
    sbl_version = _u32(region, 64 + 4 * SMEM_MASTER_SBL_VERSION_INDEX)
    if sbl_version >> 16 != SMEM_GLOBAL_PART_VERSION:
        raise SmemFormatError("unsupported SMEM global partition version")
    if not 0 <= item_id < SMEM_ITEM_COUNT:
        raise SmemFormatError("SMEM item ID is outside the kernel's supported range")

    ptable_offset = len(region) - SMEM_PTABLE_BYTES
    ptable = region[ptable_offset:]
    if ptable[:4] != SMEM_PTABLE_MAGIC:
        raise SmemFormatError("partition table magic is invalid")
    if _u32(ptable, 4) != 1:
        raise SmemFormatError("unsupported partition table version")
    count = _u32(ptable, 8)
    entries_end = SMEM_PTABLE_HEADER_SIZE + count * SMEM_PTABLE_ENTRY_SIZE
    if entries_end > len(ptable):
        raise SmemFormatError("partition table entries exceed their 4 KiB reservation")

    selected: tuple[int, int, int, int] | None = None
    for index in range(count):
        base = ptable_offset + SMEM_PTABLE_HEADER_SIZE + index * SMEM_PTABLE_ENTRY_SIZE
        offset, size = _u32(region, base), _u32(region, base + 4)
        host0, host1 = _u16(region, base + 12), _u16(region, base + 14)
        if {host0, host1} == {local_host, remote_host}:
            if selected is not None:
                raise SmemFormatError("duplicate partition for requested host pair")
            if size < SMEM_PARTITION_HEADER_SIZE or offset == 0:
                raise SmemFormatError("host-pair partition has invalid bounds")
            _range_end(offset, size, ptable_offset, "host-pair partition")
            selected = (offset, size, host0, _u32(region, base + 16))
    if selected is None:
        raise SmemFormatError("host-pair private partition was not found")

    part_offset, part_size, host0, cacheline = selected
    part = region[part_offset:part_offset + part_size]
    if part[:4] != SMEM_PART_MAGIC:
        raise SmemFormatError("private partition magic is invalid")
    part_host0, part_host1 = _u16(part, 4), _u16(part, 6)
    if {part_host0, part_host1} != {local_host, remote_host}:
        raise SmemFormatError("partition header host pair disagrees with table")
    if _u32(part, 8) != part_size:
        raise SmemFormatError("partition header size disagrees with table")
    free_uncached, free_cached = _u32(part, 12), _u32(part, 16)
    if not SMEM_PARTITION_HEADER_SIZE <= free_uncached <= free_cached <= part_size:
        raise SmemFormatError("private partition free offsets are invalid")

    cursor = SMEM_PARTITION_HEADER_SIZE
    while cursor < free_uncached:
        if free_uncached - cursor < SMEM_PRIVATE_ENTRY_SIZE:
            raise SmemFormatError("truncated private allocation header")
        canary = _u16(part, cursor)
        found_id = _u16(part, cursor + 2)
        stored_size = _u32(part, cursor + 4)
        padding_data = _u16(part, cursor + 8)
        padding_header = _u16(part, cursor + 10)
        if canary != SMEM_PRIVATE_CANARY:
            raise SmemFormatError("private allocation canary is invalid")
        data_offset = cursor + SMEM_PRIVATE_ENTRY_SIZE + padding_header
        allocation_end = _range_end(data_offset, stored_size, free_uncached,
                                    "private allocation")
        if stored_size == 0 or padding_data >= stored_size:
            raise SmemFormatError("private allocation size or padding is invalid")
        item_size = stored_size - padding_data
        if found_id == item_id:
            return SmemItem(item_id, part_offset + data_offset, item_size,
                            part_host0, part_host1)
        next_cursor = allocation_end
        if next_cursor <= cursor:
            raise SmemFormatError("private allocation list does not advance")
        cursor = next_cursor

    if cursor != free_uncached:
        raise SmemFormatError("private allocation list does not end at free offset")
    if not include_cached:
        raise SmemFormatError("item is not in the uncached allocation list")

    # Match qcom_smem_get()'s reverse walk through the cacheable allocator.
    # Each cached allocation stores its payload immediately before its header;
    # the entry and partition-header reservations are cacheline aligned.
    if free_cached < part_size:
        if cacheline < SMEM_PRIVATE_ENTRY_SIZE or cacheline & (cacheline - 1):
            raise SmemFormatError("cached allocation cacheline is invalid")
        aligned_partition_header = (
            SMEM_PARTITION_HEADER_SIZE + cacheline - 1
        ) & ~(cacheline - 1)
        aligned_entry_header = (
            SMEM_PRIVATE_ENTRY_SIZE + cacheline - 1
        ) & ~(cacheline - 1)
        if aligned_partition_header >= part_size:
            raise SmemFormatError("cached partition header exceeds partition")
        entry = part_size - aligned_partition_header
        for _ in range(SMEM_ITEM_COUNT * 8):
            if entry <= free_cached:
                break
            _range_end(entry, SMEM_PRIVATE_ENTRY_SIZE, part_size,
                       "cached allocation header")
            canary = _u16(part, entry)
            found_id = _u16(part, entry + 2)
            stored_size = _u32(part, entry + 4)
            padding_data = _u16(part, entry + 8)
            if canary != SMEM_PRIVATE_CANARY:
                raise SmemFormatError("cached allocation canary is invalid")
            if stored_size == 0 or padding_data >= stored_size:
                raise SmemFormatError("cached allocation size or padding is invalid")
            if stored_size > entry - free_cached:
                raise SmemFormatError("cached allocation payload is out of bounds")
            data_offset = entry - stored_size
            if found_id == item_id:
                return SmemItem(item_id, part_offset + data_offset,
                                stored_size - padding_data, part_host0, part_host1)
            if entry < stored_size + aligned_entry_header:
                raise SmemFormatError("cached allocation list underflows")
            next_entry = entry - stored_size - aligned_entry_header
            if next_entry >= entry:
                raise SmemFormatError("cached allocation list does not advance")
            entry = next_entry
        else:
            raise SmemFormatError("cached allocation list exceeds item bound")
    raise SmemFormatError("item was not found in private allocation lists")
