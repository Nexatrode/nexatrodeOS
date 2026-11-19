#!/usr/bin/env python3
"""Keep the temporary UFS pool probe limited to its RAM and framebuffer."""

from __future__ import annotations

import argparse
import xml.etree.ElementTree as ET
from pathlib import Path


def validate(system_path: Path) -> None:
    root = ET.parse(system_path).getroot()
    region_nodes = root.findall("memory_region")
    regions = {region.get("name"): region for region in region_nodes}
    assert len(regions) == len(region_nodes), "duplicate memory region"
    assert set(regions) == {"splash", "ufs_dma_pool"}, "unexpected memory region"
    pool = regions["ufs_dma_pool"]
    assert pool.get("size") == "0x1000000" and pool.get("page_size") == "0x1000"
    assert pool.get("phys_addr") is None, "pool base must remain build-selected"
    splash = regions["splash"]
    assert splash.get("phys_addr") == "0x9c000000"

    domains = root.findall("protection_domain")
    assert len(domains) == 1, "the probe must have one protection domain"
    domain = domains[0]
    assert domain.get("name") == "ufs_pool_probe"
    assert domain.get("smc") == "false", "the probe must have no SMC capability"
    assert domain.find("program_image").get("path") == "ufs_pool_probe.elf"
    map_nodes = domain.findall("map")
    maps = {mapping.get("mr"): mapping for mapping in map_nodes}
    assert len(maps) == len(map_nodes), "duplicate memory mapping"
    assert set(maps) == {"splash", "ufs_dma_pool"}, "unexpected device or memory mapping"
    assert maps["splash"].attrib == {
        "mr": "splash",
        "vaddr": "0x42000000",
        "perms": "rw",
        "cached": "false",
        "setvar_vaddr": "splash_fb",
    }
    assert maps["ufs_dma_pool"].attrib == {
        "mr": "ufs_dma_pool",
        "vaddr": "0x65000000",
        "perms": "rw",
        "cached": "false",
        "setvar_vaddr": "ufs_dma_pool",
    }
    setvars = [item.attrib for item in domain.findall("setvar")]
    assert setvars == [{"symbol": "ufs_dma_pool_paddr", "region_paddr": "ufs_dma_pool"}]
    assert not domain.findall("irq") and not root.findall("channel"), "no IRQ or channel"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--system", required=True, type=Path)
    args = parser.parse_args()
    validate(args.system)
    print("Pool-only profile authority passed: RAM + framebuffer only; no device capabilities")


if __name__ == "__main__":
    main()
