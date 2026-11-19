#!/usr/bin/env python3
"""Audit the bounded native SMMU experiment's actual maps, frames and IRQs."""
import argparse
import importlib.util
import json
import re
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
PROFILE = {
    'smmu_gr0': (0x15000000, 0x70000000),
    'smmu_gr1': (0x15001000, 0x70001000),
    'smmu_context': (0x15084000, 0x70002000),
    'smmu_cb_low': (0x15080000, 0x73000000),
    'smmu_cb_high': (0x15085000, 0x73010000),
    'smmu_root': (None, 0x71001000),
    'smmu_l2': (None, 0x71003000),
    'smmu_l3': (None, 0x71005000),
    'smmu_upper': (None, 0x71007000),
    'smmu_data': (None, 0x72001000),
    'splash': (0x9c000000, 0x42000000),
}


def span_size(name, data_pages=1):
    return {'splash': 0x1600000, 'smmu_cb_low': 0x4000, 'smmu_cb_high': 0x38000, 'smmu_data': data_pages * 4096}.get(name, 0x1000)


def writable(name):
    return name not in ['smmu_cb_low', 'smmu_cb_high']


class AuditError(ValueError):
    pass


def require(value, message):
    if not value:
        raise AuditError(message)


def audit(system, spec, report, dtbs=(), data_pages=1):
    require(data_pages in [1, 32] or (data_pages == 37 and
            {'smmu_l2_peer', 'smmu_l3_peer', 'smmu_l3_shared'} <= PROFILE.keys()), 'fixed experiment pool geometry')
    tree = ET.parse(system).getroot()
    regions = {r.get('name'): r for r in tree.findall('memory_region')}
    require(set(regions) == set(PROFILE), 'unexpected or missing memory region')
    require(len(tree.findall('memory_region')) == len(regions), 'duplicate region')
    pds = tree.findall('protection_domain')
    require(len(pds) == 1 and pds[0].get('name') == 'smmu_install_probe', 'one isolated PD required')
    pd = pds[0]
    require(pd.get('smc') == 'false' and not tree.findall('channel'), 'no SMC or IPC authority')
    require(int(pd.get('stack_size', '0'), 0) >= 0x100000, 'table materialization stack')
    require(len(pd.findall('map')) == len(PROFILE), 'exact map set required')
    require(len(pd.findall('setvar')) == len(PROFILE) - 1, 'exact physical symbol set')
    for name, (physical, virtual) in PROFILE.items():
        region = regions[name]
        size = span_size(name, data_pages)
        require(int(region.get('size'), 0) == size, 'region size: ' + name)
        actual = int(region.get('phys_addr'), 0) if region.get('phys_addr') else None
        require(actual == physical, 'fixed/dynamic physical binding: ' + name)
        maps = pd.findall(f"map[@mr='{name}']")
        require(len(maps) == 1 and int(maps[0].get('vaddr'), 0) == virtual
                and maps[0].get('perms') == ('rw' if writable(name) else 'r') and maps[0].get('cached') == 'false', 'map rights/address/cache: ' + name)
        require(maps[0].get('setvar_vaddr') == ('splash_fb' if name == 'splash' else name), 'virtual injection: ' + name)
        if name != 'splash':
            symbols = pd.findall(f"setvar[@symbol='{name}_paddr']")
            require(len(symbols) == 1 and symbols[0].get('region_paddr') == name, 'physical injection: ' + name)
    require(sorted((int(q.get('irq')), int(q.get('id')), q.get('trigger')) for q in pd.findall('irq'))
            == [(97, 1, 'level'), (132, 0, 'level')], 'exact fault IRQs')
    objects = spec['objects']
    allocations = {}
    for raw_id, name, raw_address in re.findall(r"^\s*(\d+) - [^:]+: '([^']+)' @ (0x[0-9a-fA-F]+)$", report, re.M):
        index, address = int(raw_id), int(raw_address, 16)
        require(index not in allocations and objects[index]['name'] == name, 'stale/duplicate report allocation')
        allocations[index] = address
    require(allocations, 'generated allocation report required')
    selected = {}
    for name in PROFILE.keys() - {'splash'}:
        frames = [(i, o) for i, o in enumerate(objects) if re.fullmatch('frame_mr_' + re.escape(name) + r'_\d+', o['name'])]
        require(len(frames) == span_size(name, data_pages) // 4096, 'real 4-KiB frame span: ' + name)
        frames.sort(key=lambda item: allocations.get(item[0], -1))
        for page, (index, obj) in enumerate(frames):
            frame = obj['object']['Frame']
            require(frame['size_bits'] == 12 and index in allocations, 'frame granule/report: ' + name)
            address = allocations[index]
            require(frame['paddr'] in [None, address], 'reported physical mismatch')
            if PROFILE[name][0] is not None:
                require(address == PROFILE[name][0] + page * 4096, 'register page physical binding')
            else:
                require(0x80000000 <= address < 1 << 40 and address % 4096 == 0, 'owned RAM frame physical bounds')
            if name == 'smmu_data':
                require(address == allocations[frames[0][0]] + page * 4096, 'contiguous private pool physical pages')
            selected[index] = (name, address, PROFILE[name][1] + page * 4096)
    require(len({address for _, address, _ in selected.values()}) == len(selected), 'physical frame alias')
    references = {i: 0 for i in selected}
    for index, obj in enumerate(objects):
        if not isinstance(obj['object'], dict):
            continue
        frame = obj['object'].get('Frame')
        if frame is not None and index in allocations:
            address, size = allocations[index], 1 << frame['size_bits']
            for chosen, (_, start, _) in selected.items():
                require(index == chosen or not (address < start + 4096 and start < address + size), 'overlapping physical frame objects')
            require(address >= 0x80000000 or index in selected, 'unexpected device frame authority')
        for container in obj['object'].values():
            if isinstance(container, dict):
                for entry in container.get('slots', []):
                    target = entry['cap'].get('Frame', {}).get('object')
                    if target in references:
                        references[target] += 1
    require(all(count == 1 for count in references.values()), 'extra retained frame capability/alias')
    roots = [i for i, obj in enumerate(objects) if isinstance(obj['object'], dict)
             and obj['object'].get('PageTable', {}).get('is_root')]
    owner = next(i for i, obj in enumerate(objects) if obj['name'] == 'pgd_smmu_install_probe')
    mappings = {i: [] for i in selected}
    guards = {v + delta for name, (_, v) in PROFILE.items() if name in ['smmu_root', 'smmu_l2', 'smmu_l3', 'smmu_upper', 'smmu_data', 'smmu_l2_peer', 'smmu_l3_peer', 'smmu_l3_shared'] for delta in [-4096, span_size(name, data_pages)]}

    def walk(root, index, level, address, ancestors):
        require(index not in ancestors and level <= 3, 'page table cycle/depth')
        table = objects[index]['object']['PageTable']
        require(table['level'] == level, 'page table level')
        for entry in table['slots']:
            at = address + (entry['slot'] << (39 - 9 * level))
            cap = entry['cap']
            if 'PageTable' in cap:
                walk(root, cap['PageTable']['object'], level + 1, at, ancestors | {index})
            elif 'Frame' in cap:
                frame_cap = cap['Frame']
                frame = objects[frame_cap['object']]['object']['Frame']
                if root == owner:
                    require(not any(at <= guard < at + (1 << frame['size_bits']) for guard in guards), 'mapped virtual guard')
                if frame_cap['object'] in selected:
                    mappings[frame_cap['object']].append((root, at, frame_cap))
    for root in roots:
        walk(root, root, 0, 0, set())
    for index, (name, _, virtual) in selected.items():
        require(len(mappings[index]) == 1, 'mapped frame alias: ' + name)
        root, at, cap = mappings[index][0]
        require(root == owner and at == virtual and cap['rights']['read'] and cap['rights']['write'] == writable(name)
                and not cap['cached'] and not cap['executable'], 'actual map authority/cache: ' + name)
    cnodes = [o['object']['CNode'] for o in objects if o['name'] == 'cnode_smmu_install_probe']
    require(len(cnodes) == 1, 'owner CNode')
    slots = {entry['slot']: entry['cap'] for entry in cnodes[0]['slots']}
    require(not any('smc' in kind.lower() for cap in slots.values() for kind in cap), 'SMC capability present')
    require(sorted(q['irq'] for q in spec['irqs']) == [97, 132], 'extra or missing IRQ authority')
    for channel, irq in [(0, 132), (1, 97)]:
        handler = next(q['handler'] for q in spec['irqs'] if q['irq'] == irq)
        require(slots[138 + channel]['ArmIrqHandler']['object'] == handler, 'IRQ channel binding')
        description = objects[handler]['object']['ArmIrq']
        notice = description['slots'][0]['cap']['Notification']
        require(notice['object'] == slots[1]['Notification']['object'] and notice['badge'] == 1 << channel
                and description['extra']['trigger'] == 0, 'fault IRQ destination/trigger')
    module_spec = importlib.util.spec_from_file_location('memory_audit', Path(__file__).with_name('audit-capdl-memory.py'))
    memory = importlib.util.module_from_spec(module_spec); module_spec.loader.exec_module(memory)
    for dtb in dtbs:
        exclusions, _ = memory.read_regions(dtb)
        for name, address, _ in selected.values():
            if PROFILE[name][0] is None:
                for region, start, size, _ in exclusions:
                    require(not (address < start + size and start < address + 4096), 'RAM frame overlaps ' + region)
    return {'scope': 'one unused route/context 4; other contexts read-only; no SMC, radio or PAS capability',
            'private_data_pages': data_pages, 'private_data_bytes': data_pages * 4096,
            'frames': {name: [hex(address) for n, address, _ in selected.values() if n == name] for name in PROFILE if name != 'splash'},
            'fault_irqs': [97, 132], 'guard_pages_unmapped': len(guards)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--system', type=Path, default=Path(__file__).with_name('smmu-install-probe.system'))
    parser.add_argument('--capdl', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--dt', type=Path, action='append', default=[])
    parser.add_argument('--output', type=Path)
    parser.add_argument('--data-pages', type=int, choices=[1, 32], default=1)
    args = parser.parse_args()
    result = audit(args.system, json.loads(args.capdl.read_text()), args.report.read_text(), args.dt, args.data_pages)
    text = json.dumps(result, indent=2) + '\n'
    if args.output:
        args.output.write_text(text)
    print(text, end='')


if __name__ == '__main__':
    main()
