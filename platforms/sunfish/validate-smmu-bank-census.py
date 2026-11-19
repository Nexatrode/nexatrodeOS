#!/usr/bin/env python3
"""Verify the actual census image has exclusively read-only SMMU authority."""
import argparse
import json
import re
from pathlib import Path
import xml.etree.ElementTree as ET


def require(value, message):
    if not value:
        raise ValueError(message)


def audit(model, report, system):
    tree = ET.parse(system).getroot()
    regions = tree.findall('memory_region')
    require(len(regions) == 2, 'exactly SMMU and splash regions')
    properties = {r.get('name'): (int(r.get('phys_addr'), 0), int(r.get('size'), 0)) for r in regions}
    require(properties == {'apps_smmu': (0x15000000, 0x100000), 'splash': (0x9c000000, 0x1600000)}, 'physical profile')
    domains = tree.findall('protection_domain')
    require(len(domains) == 1, 'one PD')
    pd = domains[0]
    require(pd.get('name') == 'smmu_read_probe' and pd.get('smc') == 'false', 'isolated non-SMC PD')
    require(not pd.findall('irq') and not tree.findall('channel') and not model['irqs'], 'no IRQ/IPC authority')
    maps = pd.findall('map')
    require(len(maps) == 2, 'exact maps')
    require({m.get('mr'): (int(m.get('vaddr'), 0), m.get('perms'), m.get('cached')) for m in maps}
            == {'apps_smmu': (0x40000000, 'r', 'false'), 'splash': (0x50000000, 'rw', 'false')}, 'XML mapping rights')
    objects = model['objects']
    addresses = {int(i): int(address, 16) for i, address in re.findall(r"^\s*(\d+) - [^:]+: '[^']+' @ (0x[0-9a-fA-F]+)$", report, re.M)}
    selected = {}
    for i, obj in enumerate(objects):
        if obj['name'].startswith('frame_mr_apps_smmu_'):
            frame = obj['object']['Frame']
            require(frame['size_bits'] == 12 and i in addresses, '4-KiB SMMU frame')
            require(frame['paddr'] == addresses[i], 'physical frame identity')
            selected[i] = addresses[i]
    require(sorted(selected.values()) == list(range(0x15000000, 0x15100000, 0x1000)), 'complete exact SMMU aperture')
    for i, obj in enumerate(objects):
        if not isinstance(obj['object'], dict) or 'Frame' not in obj['object'] or i not in addresses:
            continue
        frame = obj['object']['Frame']
        start, end = addresses[i], addresses[i] + (1 << frame['size_bits'])
        require(start >= 0x80000000 or i in selected, 'unexpected device frame authority')
        require(not (start < 0x15100000 and 0x15000000 < end) or i in selected, 'physical SMMU frame alias')
    references = {i: 0 for i in selected}
    for obj in objects:
        if isinstance(obj['object'], dict):
            for container in obj['object'].values():
                if isinstance(container, dict):
                    for entry in container.get('slots', []):
                        cap = entry['cap']
                        require(not any('smc' in kind.lower() for kind in cap), 'SMC cap')
                        frame = cap.get('Frame')
                        if frame and frame['object'] in selected:
                            references[frame['object']] += 1
                            require(frame['rights']['read'] and not frame['rights']['write']
                                    and not frame['executable'] and not frame['cached'], 'non-read-only SMMU cap')
    require(all(n == 1 for n in references.values()), 'SMMU frame cap aliases')
    owner = next(i for i, obj in enumerate(objects) if obj['name'] == 'pgd_smmu_read_probe')
    found = {}

    def walk(root, index, level, address, ancestors):
        require(index not in ancestors and level <= 3, 'page-table cycle/depth')
        table = objects[index]['object']['PageTable']
        require(table['level'] == level, 'page table level')
        for entry in table['slots']:
            at = address + (entry['slot'] << (39 - 9 * level))
            cap = entry['cap']
            if 'PageTable' in cap:
                walk(root, cap['PageTable']['object'], level + 1, at, ancestors | {index})
            elif 'Frame' in cap and cap['Frame']['object'] in selected:
                target = cap['Frame']['object']
                require(target not in found and root == owner and at == 0x40000000 + selected[target] - 0x15000000, 'SMMU virtual alias/owner')
                found[target] = at
    for i, obj in enumerate(objects):
        if isinstance(obj['object'], dict) and obj['object'].get('PageTable', {}).get('is_root'):
            walk(i, i, 0, 0, set())
    require(set(found) == set(selected), 'unmapped SMMU frame')
    return {'scope': 'read-only SMMU bank census; no SMC, IRQ, IPC or DMA capability', 'readonly_register_pages': len(selected), 'register_writes_possible': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--capdl', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    result = audit(json.loads(args.capdl.read_text()), args.report.read_text(), Path(__file__).with_name('smmu-read-probe.system'))
    text = json.dumps(result, indent=2) + '\n'
    if args.output:
        args.output.write_text(text)
    print(text, end='')


if __name__ == '__main__':
    main()
