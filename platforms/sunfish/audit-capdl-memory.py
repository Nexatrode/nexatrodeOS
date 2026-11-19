#!/usr/bin/env python3
"""Check generated Microkit pages against fixed reserved-memory DT ranges."""

from __future__ import annotations

import argparse
import hashlib
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DTB = (
    ROOT
    / ".cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb"
)
DEFAULT_REPORT = ROOT / ".cache/pixel-sunfish/slint-display/report.txt"
UNITS = {"B": 1, "KiB": 1024, "MiB": 1024**2, "GiB": 1024**3}
PAGE_RE = re.compile(
    r"Page\((\d+)\s+(B|KiB|MiB|GiB)\):\s+'([^']+)'\s+@\s+(0x[0-9a-fA-F]+)"
)


def fdtget(dtb: Path, *args: str) -> str:
    return subprocess.check_output(["fdtget", str(dtb), *args], text=True).strip()


def read_cells(dtb: Path, path: str, prop: str) -> list[int]:
    values = fdtget(dtb, "-t", "x", path, prop)
    return [int(value, 16) for value in values.split()]


def read_regions(dtb: Path) -> tuple[list[tuple[str, int, int, bool]], list[str]]:
    root = "/reserved-memory"
    address_cells = int(fdtget(dtb, "-t", "x", root, "#address-cells"), 16)
    size_cells = int(fdtget(dtb, "-t", "x", root, "#size-cells"), 16)
    if address_cells < 1 or size_cells < 1:
        raise ValueError("invalid reserved-memory cell widths")

    regions: list[tuple[str, int, int, bool]] = []
    dynamic: list[str] = []
    names = fdtget(dtb, "-l", root).splitlines()
    for name in names:
        path = f"{root}/{name}"
        props = set(fdtget(dtb, "-p", path).splitlines())
        if "reg" not in props:
            if "size" in props:
                size = sum(
                    read_cells(dtb, path, "size")[-size_cells + i]
                    << (32 * (size_cells - i - 1))
                    for i in range(size_cells)
                )
                dynamic.append(f"{name} (size 0x{size:x}, no fixed reg)")
            continue

        cells = read_cells(dtb, path, "reg")
        stride = address_cells + size_cells
        if len(cells) == 0 or len(cells) % stride:
            raise ValueError(f"malformed reg cells at {path}: {cells}")
        for offset in range(0, len(cells), stride):
            address = sum(
                cells[offset + i] << (32 * (address_cells - i - 1))
                for i in range(address_cells)
            )
            size = sum(
                cells[offset + address_cells + i]
                << (32 * (size_cells - i - 1))
                for i in range(size_cells)
            )
            regions.append((name, address, size, "no-map" in props))
    return regions, dynamic


def read_pages(report: Path) -> list[tuple[int, int, str]]:
    pages = []
    for line in report.read_text().splitlines():
        match = PAGE_RE.search(line)
        if match:
            size = int(match.group(1)) * UNITS[match.group(2)]
            pages.append((int(match.group(4), 16), size, match.group(3)))
    if not pages:
        raise ValueError(f"no CapDL page objects found in {report}")
    return pages


def read_only_cmd_db_pages(system: Path | None) -> set[tuple[int, int]]:
    """Opt-in exception for exactly two observed DB pages, mapped read-only.

    No other XBL/AOP memory is permitted. The caller must supply the same
    manifest used by Microkit, not an unrelated proof of a read-only mapping.
    """
    if system is None:
        return set()
    root = ET.parse(system).getroot()
    dictionaries = root.findall("memory_region[@name='cmd_db_dictionary']")
    if len(dictionaries) != 1:
        raise ValueError("expected one command DB dictionary page")
    dictionary = dictionaries[0]
    if (int(dictionary.get("phys_addr", "0"), 0), int(dictionary.get("size", "0"), 0),
            int(dictionary.get("page_size", "0"), 0)) != (0x0c3f0000, 0x1000, 0x1000):
        raise ValueError("command DB dictionary exception is restricted to its observed page")
    dictionary_maps = root.findall(".//map[@mr='cmd_db_dictionary']")
    if not dictionary_maps or any(node.get("perms") != "r" or node.get("cached") != "false"
                                  for node in dictionary_maps):
        raise ValueError("command DB dictionary mappings must be uncached and read-only")
    regions = root.findall("memory_region[@name='cmd_db_snapshot']")
    if len(regions) != 1:
        raise ValueError("expected one command DB snapshot region")
    region = regions[0]
    if (int(region.get("phys_addr", "0"), 0), int(region.get("size", "0"), 0),
            int(region.get("page_size", "0"), 0)) != (0x85f20000, 0x2000, 0x1000):
        raise ValueError("command DB exception is restricted to observed two-page window")
    mappings = root.findall(".//map[@mr='cmd_db_snapshot']")
    if not mappings or any(node.get("perms") != "r" or node.get("cached") != "false"
                           for node in mappings):
        raise ValueError("command DB probe mappings must be uncached and read-only")
    return {(0x85f20000, 0x1000), (0x85f21000, 0x1000)}


def fixed_modem_data_pages(system: Path | None) -> set[tuple[int, int]]:
    """Explicit exception for the isolated segment23 experiment, never general RAM.

    Pin the entire manifest to the existing MEM_SETUP probe plus one exact RW,
    NX, uncached 4K page. No aliases, clients, extra regions or broader window.
    """
    if system is None:
        return set()
    root = ET.parse(system).getroot()
    expected = ET.parse(Path(__file__).with_name('pas-mem-setup-probe.system')).getroot()
    pd = expected.find('protection_domain')
    pd.set('name', 'modem_data_probe')
    pd.find('program_image').set('path', 'modem_data_probe.elf')
    # XML order is immaterial; every node, attribute and multiplicity is pinned.
    ET.SubElement(expected, 'memory_region', name='modem_data_page',
                  size='0x1000', page_size='0x1000', phys_addr='0x93200000')
    ET.SubElement(pd, 'map', mr='modem_data_page', vaddr='0x40020000',
                  perms='rw', cached='false', setvar_vaddr='modem_data_page')
    def shape(node):
        return (node.tag, tuple(sorted(node.attrib.items())),
                tuple(sorted(shape(child) for child in node)))
    if shape(root) != shape(expected):
        raise ValueError('modem data exception requires the exact isolated segment23 manifest')
    return {(0x93200000, 0x1000)}


def is_modem_data_page(page, allowed):
    base, size, name = page
    return (base, size) in allowed and name.startswith('frame_mr_modem_data_page_')


def fixed_modem_segment_pages(system: Path | None) -> set[tuple[int, int, str]]:
    """Segment14 only: two RW/NX pages between two read-only guard pages."""
    if system is None:
        return set()
    root = ET.parse(system).getroot()
    expected = ET.parse(Path(__file__).with_name('pas-mem-setup-probe.system')).getroot()
    pd = expected.find('protection_domain')
    pd.set('name', 'modem_segment_probe')
    pd.find('program_image').set('path', 'modem_segment_probe.elf')
    allowed = set()
    for name, base, size, virtual, perms in [
        ('modem_guard_low', '0x8b77f000', '0x1000', '0x40020000', 'r'),
        ('modem_segment_data', '0x8b780000', '0x2000', '0x40021000', 'rw'),
        ('modem_guard_high', '0x8b782000', '0x1000', '0x40023000', 'r'),
    ]:
        ET.SubElement(expected, 'memory_region', name=name, size=size,
                      page_size='0x1000', phys_addr=base)
        attrs = dict(mr=name, vaddr=virtual, perms=perms, cached='false')
        if name == 'modem_guard_low':
            attrs['setvar_vaddr'] = 'modem_segment_window'
        ET.SubElement(pd, 'map', **attrs)
        for offset in range(0, int(size, 0), 4096):
            allowed.add((int(base, 0) + offset, 4096, 'frame_mr_' + name + '_'))
    def shape(node):
        return (node.tag, tuple(sorted(node.attrib.items())),
                tuple(sorted(shape(child) for child in node)))
    if shape(root) != shape(expected):
        raise ValueError('modem segment exception requires the exact isolated segment14 manifest')
    return allowed


def is_modem_segment_page(page, allowed):
    base, size, name = page
    return any((base, size) == (start, length) and name.startswith(prefix)
               for start, length, prefix in allowed)


def validate_modem_segment_frames(pages, allowed):
    chosen = [p for p in pages if is_modem_segment_page(p, allowed)]
    if len(chosen) != 4 or {(p[0], p[1]) for p in chosen} != {(p[0], p[1]) for p in allowed}:
        raise ValueError('expected exactly four fixed segment14/guard frames, without aliases')
    return chosen


def fixed_modem_image_pages(system: Path | None, observe_ready: bool = False, handshake: bool = False, glink_prepare: bool = False, fatal_trace: bool = False, servreg_state_probe: bool = False) -> set[tuple[int, int]]:
    """Whole image staging only: one trusted domain, fixed pool, private gap RAM."""
    if system is None:
        return set()
    if glink_prepare and (not handshake or observe_ready):
        raise ValueError('fresh GLINK memory preparation requires its separate AP writer mode')
    if fatal_trace and (not handshake or observe_ready):
        raise ValueError('fatal trace requires the existing modem handshake/read-only transport mode')
    if servreg_state_probe:
        if not handshake or observe_ready or not system.is_file():
            raise ValueError('SERVREG state query requires the exact owner handshake manifest')
        expected_hash = 'cc2f0d7e8ef4cf308ff8c864aadb749847f59f07d29a40bf488afd31be69d083'
        if hashlib.sha256(system.read_bytes()).hexdigest() != expected_hash:
            raise ValueError('SERVREG state-query least-privilege manifest changed')
        return {(base, 0x200000) for base in range(0x8b000000, 0x94800000, 0x200000)}
    root = ET.parse(system).getroot()
    expected = ET.parse(Path(__file__).with_name('pas-mem-setup-probe.system')).getroot()
    pd = expected.find('protection_domain')
    pd.set('name', 'modem_image_probe')
    pd.find('program_image').set('path', 'modem_image_probe.elf')
    ET.SubElement(expected, 'memory_region', name='modem_image_pool',
                  size='0x9800000', page_size='0x200000', phys_addr='0x8b000000')
    ET.SubElement(expected, 'memory_region', name='modem_gap_snapshot',
                  size='0x200000', page_size='0x200000')
    ET.SubElement(pd, 'map', mr='modem_image_pool', vaddr='0x50000000',
                  perms='rw', cached='false', setvar_vaddr='modem_image_pool')
    ET.SubElement(pd, 'map', mr='modem_gap_snapshot', vaddr='0x60000000',
                  perms='rw', cached='true', setvar_vaddr='modem_gap_snapshot')
    if observe_ready and handshake:
        raise ValueError('choose passive observer or AP status writer')
    if handshake:
        smem_maps=[
            ('smem_snapshot_low','0x86000000','0x11d000','0x44000000','r','modem_smem_status'),
            ('smem_ap_page','0x8611d000','0x1000','0x4411d000','rw','smp2p_ap_page'),
            ('smem_snapshot_high','0x8611e000','0xe2000','0x4411e000','r',None),
        ]
        if glink_prepare:
            smem_maps[-1:]=[
                ('glink_prepare_tail','0x8611e000','0x4000','0x4411e000','rw','glink_prepare_tail'),
                ('smem_snapshot_high','0x86122000','0xde000','0x44122000','r',None),
            ]
        for name,base,size,virtual,perms,symbol in smem_maps:
            ET.SubElement(expected,'memory_region',name=name,size=size,page_size='0x1000',phys_addr=base)
            attrs=dict(mr=name,vaddr=virtual,perms=perms,cached='false')
            if symbol:attrs['setvar_vaddr']=symbol
            ET.SubElement(pd,'map',**attrs)
    if observe_ready or handshake:
        if observe_ready:
            ET.SubElement(expected, 'memory_region', name='smem_snapshot', size='0x200000',
                          page_size='0x1000', phys_addr='0x86000000')
        ET.SubElement(expected, 'memory_region', name='tcsr_smem_lock', size='0x1000',
                      page_size='0x1000', phys_addr='0x01f43000')
        if observe_ready:
            ET.SubElement(pd, 'map', mr='smem_snapshot', vaddr='0x44000000', perms='r',
                          cached='false', setvar_vaddr='modem_smem_status')
        ET.SubElement(pd, 'map', mr='tcsr_smem_lock', vaddr='0x40006000', perms='rw',
                      cached='false', setvar_vaddr='tcsr_smem_lock')
    def shape(node):
        return (node.tag, tuple(sorted(node.attrib.items())),
                tuple(sorted(shape(child) for child in node)))
    if shape(root) != shape(expected):
        raise ValueError('modem image exception requires the exact isolated image-staging manifest')
    return {(base, 0x200000) for base in range(0x8b000000, 0x94800000, 0x200000)}


def is_modem_image_page(page, allowed):
    base, size, name = page
    return (base, size) in allowed and name.startswith('frame_mr_modem_image_pool_')


def validate_modem_image_frames(pages, allowed):
    chosen = [p for p in pages if is_modem_image_page(p, allowed)]
    if len(chosen) != 76 or {(p[0], p[1]) for p in chosen} != allowed:
        raise ValueError('expected exactly76 fixed2MiB modem frames, without aliases')
    return chosen


def fixed_modem_fatal_trace_pages(system: Path | None, fatal_trace: bool) -> set[tuple[int,int]]:
    if not fatal_trace:
        return set()
    fixed_modem_image_pages(system,False,True,True,True)
    return set()


def validate_modem_fatal_trace_frames(pages, allowed):
    chosen=[p for p in pages if p[2].startswith('frame_mr_gicd_watchdog_status_')]
    if not allowed:
        if chosen: raise ValueError('unexpected GICD page without fatal-trace mode')
        return []
    if len(chosen)!=1 or {(p[0],p[1]) for p in chosen}!=allowed:
        raise ValueError('fatal trace requires exactly one fixed GICD page')
    return chosen


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dtb", type=Path, default=DEFAULT_DTB)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--read-only-cmd-db-system", type=Path,
                        help="same build manifest: permit only read-only observed command DB pages")
    parser.add_argument("--fixed-modem-data-system", type=Path,
                        help="same build manifest: permit only the fixed isolated segment23 page")
    parser.add_argument("--fixed-modem-segment-system", type=Path,
                        help="same build manifest: permit only segment14 and two read-only guards")
    parser.add_argument("--fixed-modem-image-system", type=Path,
                        help="same build manifest: permit only the fixed whole-image staging pool")
    parser.add_argument('--observe-modem-ready',action='store_true')
    parser.add_argument('--handshake-modem-status',action='store_true')
    parser.add_argument('--prepare-glink-memory',action='store_true')
    parser.add_argument('--fatal-trace',action='store_true')
    parser.add_argument('--servreg-state-probe',action='store_true')
    args = parser.parse_args()
    if not args.dtb.is_file() or not args.report.is_file():
        parser.error(f"missing DTB or CapDL report: {args.dtb}, {args.report}")

    regions, dynamic = read_regions(args.dtb)
    pages = read_pages(args.report)
    cmd_db_pages = read_only_cmd_db_pages(args.read_only_cmd_db_system)
    modem_pages = fixed_modem_data_pages(args.fixed_modem_data_system)
    segment_pages = fixed_modem_segment_pages(args.fixed_modem_segment_system)
    if (args.observe_modem_ready or args.handshake_modem_status or args.prepare_glink_memory or args.fatal_trace) and args.fixed_modem_image_system is None:
        raise ValueError('status observer requires the exact whole-image manifest')
    image_pages = fixed_modem_image_pages(args.fixed_modem_image_system,args.observe_modem_ready,args.handshake_modem_status,args.prepare_glink_memory,args.fatal_trace,args.servreg_state_probe)
    trace_pages=fixed_modem_fatal_trace_pages(args.fixed_modem_image_system,args.fatal_trace)
    trace_page_objects=validate_modem_fatal_trace_frames(pages,trace_pages)
    if sum(bool(p) for p in [modem_pages, segment_pages, image_pages]) > 1:
        raise ValueError('modem experiment exceptions are mutually exclusive')
    if segment_pages:
        validate_modem_segment_frames(pages, segment_pages)
    if image_pages:
        validate_modem_image_frames(pages, image_pages)
    if modem_pages and sum(is_modem_data_page(p, modem_pages) for p in pages) != 1:
        raise ValueError('expected exactly one fixed modem data frame')
    violations = []
    allowed_splash = 0
    allowed_smem_probe = 0
    allowed_cmd_db_probe = 0
    allowed_modem_data_probe = 0
    allowed_modem_segment_probe = 0
    allowed_modem_image_probe = 0
    allowed_unmapped_msa_reservation = 0
    for name, base, size, no_map in regions:
        end = base + size
        for page_base, page_size, page_name in pages:
            page_end = page_base + page_size
            if page_base >= end or page_end <= base:
                continue
            intended_splash = (
                name == "cont_splash_region@9c000000"
                and page_name.startswith("frame_mr_splash_")
                and page_base >= base
                and page_end <= end
            )
            intended_smem_probe = (
                name == "smem@86000000"
                and (page_name.startswith("frame_mr_smem_snapshot_") or
                     (args.handshake_modem_status and page_name.startswith("frame_mr_smem_ap_page_")
                      and page_base==0x8611d000 and page_size==4096) or
                     (args.prepare_glink_memory and page_name.startswith('frame_mr_glink_prepare_tail_')
                      and page_base in range(0x8611e000,0x86122000,4096) and page_size==4096))
                and page_base >= base
                and page_end <= end
            )
            intended_cmd_db_probe = (
                name == "xbl_aop_mem@85e00000"
                and page_name.startswith("frame_mr_cmd_db_snapshot_")
                and (page_base, page_size) in cmd_db_pages
            )
            intended_modem_data_probe = (
                name == 'modem_region@8b000000'
                and no_map
                and (base, size) == (0x8b000000, 0x09800000)
                and is_modem_data_page((page_base, page_size, page_name), modem_pages)
            )
            intended_modem_segment_probe = (
                name == 'modem_region@8b000000'
                and no_map
                and (base, size) == (0x8b000000, 0x09800000)
                and is_modem_segment_page((page_base, page_size, page_name), segment_pages)
            )
            intended_modem_image_probe = (
                name == 'modem_region@8b000000'
                and no_map
                and (base, size) == (0x8b000000, 0x09800000)
                and is_modem_image_page((page_base, page_size, page_name), image_pages)
            )
            intended_unmapped_msa_reservation = (
                args.servreg_state_probe
                and name == 'wlan_msa_region@97500000'
                and no_map
                and (base, size) == (0x98d00000, 0x200000)
                and page_name.startswith('frame_mr_wlan_msa_pool_')
                and page_base >= base
                and page_end <= end
            )
            if intended_splash:
                allowed_splash += 1
            elif intended_smem_probe:
                allowed_smem_probe += 1
            elif intended_cmd_db_probe:
                allowed_cmd_db_probe += 1
            elif intended_modem_data_probe:
                allowed_modem_data_probe += 1
            elif intended_modem_segment_probe:
                allowed_modem_segment_probe += 1
            elif intended_modem_image_probe:
                allowed_modem_image_probe += 1
            elif intended_unmapped_msa_reservation:
                allowed_unmapped_msa_reservation += 1
            else:
                kind = "no-map" if no_map else "reserved"
                violations.append(
                    f"{page_name} [{page_base:#x}, {page_end:#x}) overlaps "
                    f"{kind} {name} [{base:#x}, {end:#x})"
                )

    print(
        f"CapDL pages: {len(pages)}; fixed reserved intervals: {len(regions)}; "
        f"intentional splash pages: {allowed_splash}; "
        f"SMEM probe pages: {allowed_smem_probe}; "
        f"read-only command DB pages: {allowed_cmd_db_probe}; "
        f"fixed modem data pages: {allowed_modem_data_probe}; "
        f"fixed segment/guard pages: {allowed_modem_segment_probe}; "
        f"fixed whole-image pages: {allowed_modem_image_probe}; "
        f"unmapped WLAN MSA reservation pages: {allowed_unmapped_msa_reservation}; "
        f"read-only GICD trace pages: {len(trace_page_objects)}; overlaps: {len(violations)}"
    )
    if dynamic:
        print("Dynamic reserved-memory nodes without fixed addresses:")
        for entry in dynamic:
            print(f"  {entry}")
    if violations:
        print("Reserved-memory overlaps:", file=sys.stderr)
        for violation in violations:
            print(f"  {violation}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
