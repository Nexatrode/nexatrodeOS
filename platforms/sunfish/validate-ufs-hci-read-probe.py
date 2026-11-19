#!/usr/bin/env python3
"""Keep the temporary UFSHCI snapshot limited to RO controller MMIO + display."""

from __future__ import annotations

import argparse
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def validate(system_path: Path) -> None:
    root = ET.parse(system_path).getroot()
    regions_list = root.findall("memory_region")
    regions = {region.get("name"): region for region in regions_list}
    assert len(regions) == len(regions_list), "duplicate memory region"
    assert set(regions) == {"ufs_hci_mmio", "splash"}, "unexpected memory region"
    hci = regions["ufs_hci_mmio"]
    assert hci.attrib == {
        "name": "ufs_hci_mmio",
        "size": "0x3000",
        "page_size": "0x1000",
        "phys_addr": "0x01d84000",
    }
    splash = regions["splash"]
    assert splash.attrib == {
        "name": "splash",
        "size": "0x1600000",
        "page_size": "0x200000",
        "phys_addr": "0x9c000000",
    }

    domains = root.findall("protection_domain")
    assert len(domains) == 1, "the probe must have one protection domain"
    domain = domains[0]
    assert domain.get("name") == "ufs_hci_read_probe"
    assert domain.get("smc") == "false", "the probe must have no SMC capability"
    assert domain.find("program_image").get("path") == "ufs_hci_read_probe.elf"
    maps_list = domain.findall("map")
    maps = {mapping.get("mr"): mapping for mapping in maps_list}
    assert len(maps) == len(maps_list), "duplicate memory mapping"
    assert set(maps) == {"ufs_hci_mmio", "splash"}, "unexpected MMIO or RAM mapping"
    assert maps["ufs_hci_mmio"].attrib == {
        "mr": "ufs_hci_mmio",
        "vaddr": "0x64000000",
        "perms": "r",
        "cached": "false",
        "setvar_vaddr": "ufs_hci_mmio",
    }
    assert maps["splash"].attrib == {
        "mr": "splash",
        "vaddr": "0x42000000",
        "perms": "rw",
        "cached": "false",
        "setvar_vaddr": "splash_fb",
    }
    assert not domain.findall("irq") and not root.findall("channel"), "no IRQ or channel"
    assert not domain.findall("setvar"), "no DMA or physical-address authority"

    source = (ROOT / "rust/src/sunfish_ufs_hci_read_probe.rs").read_text()
    assert "sel4_microkit::var!(ufs_hci_mmio: usize = 0)" in source
    assert "memory_region_symbol!(ufs_hci_mmio" not in source
    init = source[source.index("#[protection_domain]") :]
    ordered = (
        "sel4_microkit::var!(ufs_hci_mmio: usize = 0)",
        "if hci_vaddr != 0x6400_0000",
        "let base = hci_vaddr as *const u32",
        'checkpoint_read(&display, base, REG_CAPABILITIES, "READ CAP")',
        'checkpoint_read(&display, base, REG_VERSION, "READ VERSION")',
        'checkpoint_read(&display, base, REG_CONTROLLER_STATUS, "READ HCS")',
    )
    positions = tuple(init.find(needle) for needle in ordered)
    assert all(position >= 0 for position in positions), "missing HCI mapping preflight"
    assert positions == tuple(sorted(positions)), "HCI read occurs before mapping validation"
    assert 'display.line(4, "NO HCI READ")' in init
    checkpoint = source[source.index("fn checkpoint_read") : source.index("#[protection_domain]")]
    assert checkpoint.index('display.replace_line(1, label)') < checkpoint.index("read_register(base, offset)")
    assert checkpoint.index("read_register(base, offset)") < checkpoint.index("display.hex(4, label, value)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--system", required=True, type=Path)
    args = parser.parse_args()
    validate(args.system)
    print("UFSHCI probe authority passed: RO HCI MMIO + splash only")


if __name__ == "__main__":
    main()
