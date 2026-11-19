#!/usr/bin/env python3
"""Validate reserved service frames, guard holes and generated capabilities.

Reads cached DT/build artifacts only. An allocator/mapping proof is not a
QHEE HLOS ownership or SCM ABI proof; native services remain disabled.
"""
import argparse
import importlib.util
import json
import re
from pathlib import Path
import struct
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
POOLS = {
    'modem_storage': (0xa2001000, 0x200000, 0x61001000),
    'modem_heap': (0xa2401000, 0x500000, 0x62001000),
    'modem_scm_args': (0xa2a01000, 0x1000, 0x63001000),
}
WLAN_MSA = (0x98d00000, 0x200000)


def spans():
    for name, (base, size, virtual) in POOLS.items():
        yield name, base, size, virtual, True
        yield name + '_guard_low', base - 4096, 4096, virtual - 4096, False
        yield name + '_guard_high', base + size, 4096, virtual + size, False


def overlap(a, n, b, m):
    return a < b + m and b < a + n


def validate_manifest(tree):
    if isinstance(tree, (str, Path)):
        tree = ET.parse(tree).getroot()
    regions = tree.findall('memory_region')
    maps = tree.findall('.//map')
    for name, base, size, virtual, mapped in spans():
        mr = [r for r in regions if r.get('name') == name]
        assert len(mr) == 1, 'one reservation: ' + name
        r = mr[0]
        assert (int(r.get('phys_addr', '0'), 0), int(r.get('size'), 0), int(r.get('page_size'), 0)) == (base, size, 4096), name
        uses = [m for m in maps if m.get('mr') == name]
        assert len(uses) == int(mapped), 'guard mapped or duplicate/missing mapping: ' + name
        if mapped:
            m = uses[0]
            assert m in tree.find('protection_domain').findall('map'), 'wrong pool owner'
            assert (int(m.get('vaddr'), 0), m.get('perms'), m.get('cached'), m.get('setvar_vaddr')) == (virtual, 'rw', 'false', name), name
            sv = tree.findall(f"protection_domain/setvar[@symbol='{name}_paddr']")
            assert len(sv) == 1 and sv[0].get('region_paddr') == name, 'pool physical symbol'
        for other in regions:
            if other is r or not other.get('phys_addr'):
                continue
            assert not overlap(base, size, int(other.get('phys_addr'), 0), int(other.get('size'), 0)), 'physical reservation alias'
        for m in maps:
            if m in uses:
                continue
            others = [r for r in regions if r.get('name') == m.get('mr')]
            assert len(others) == 1, 'unknown mapping'
            assert not overlap(virtual, size, int(m.get('vaddr'), 0), int(others[0].get('size'), 0)), 'pool/guard virtual alias'
    msa_regions = [r for r in regions if r.get('name') == 'wlan_msa_pool']
    assert len(msa_regions) == 1, 'one fixed WLAN MSA reservation'
    msa = msa_regions[0]
    assert (int(msa.get('phys_addr', '0'), 0), int(msa.get('size'), 0), int(msa.get('page_size'), 0)) == (
        WLAN_MSA[0], WLAN_MSA[1], 4096
    ), 'WLAN MSA physical profile'
    assert not [m for m in maps if m.get('mr') == 'wlan_msa_pool'], 'WLAN MSA must remain unmapped'
    for other in regions:
        if other is msa or not other.get('phys_addr'):
            continue
        assert not overlap(WLAN_MSA[0], WLAN_MSA[1], int(other.get('phys_addr'), 0), int(other.get('size'), 0)), 'WLAN MSA physical reservation alias'


def validate_capdl(spec):
    objects = spec['objects']
    expected = {}
    for name, base, size, virtual, mapped in spans():
        for offset in range(0, size, 4096):
            expected[base+offset] = (name, virtual+offset, mapped)
    chosen = {}
    msa_start, msa_size = WLAN_MSA
    msa_expected = set(range(msa_start, msa_start + msa_size, 4096))
    msa_chosen = {}
    for i, obj in enumerate(objects):
        if not isinstance(obj['object'], dict):
            continue
        frame = obj['object'].get('Frame')
        if frame is None or frame.get('paddr') is None:
            continue
        base, size = frame['paddr'], 1 << frame['size_bits']
        if overlap(base, size, msa_start, msa_size):
            assert size == 4096 and base in msa_expected and base not in msa_chosen, 'WLAN MSA frame overlap/alias'
            assert obj['name'].startswith('frame_mr_wlan_msa_pool_'), 'wrong WLAN MSA frame reservation'
            msa_chosen[base] = i
            continue
        hits = [p for p in expected if overlap(base, size, p, 4096)]
        if not hits:
            continue
        assert size == 4096 and base in expected and base not in chosen, 'pool frame overlap/alias'
        name, virtual, mapped = expected[base]
        assert obj['name'].startswith('frame_mr_' + name + '_'), 'wrong allocated frame'
        chosen[base] = i
    assert chosen.keys() == expected.keys(), 'incomplete reserved physical pages/guards'
    assert msa_chosen.keys() == msa_expected, 'incomplete fixed WLAN MSA frames'
    refs = {i: [] for i in (*chosen.values(), *msa_chosen.values())}
    for obj in objects:
        if not isinstance(obj['object'], dict):
            continue
        for kind, container in obj['object'].items():
            if not isinstance(container, dict) or 'slots' not in container:
                continue
            for slot in container['slots']:
                f = slot['cap'].get('Frame')
                if f is not None and f['object'] in refs:
                    refs[f['object']].append((obj, kind, slot['slot'], f))
    owner_roots = [i for i, o in enumerate(objects) if o.get('name') == 'pgd_modem_image_probe']
    assert len(owner_roots) == 1, 'one actual owner VSpace'
    assert all(not refs[i] for i in msa_chosen.values()), 'WLAN MSA frame capability exposed'
    root_refs = {i: [] for i in chosen.values()}
    def walk(root, obj_id, level, address, ancestors):
        assert obj_id not in ancestors and level <= 3, 'page-table cycle/depth'
        table = objects[obj_id]['object']['PageTable']
        assert table['level'] == level, 'page-table level'
        slots = table['slots']
        assert len({s['slot'] for s in slots}) == len(slots), 'duplicate page-table slot'
        for slot in slots:
            assert 0 <= slot['slot'] < 512, 'page-table slot bounds'
            at = address + (slot['slot'] << (39 - 9*level))
            if 'PageTable' in slot['cap']:
                walk(root, slot['cap']['PageTable']['object'], level+1, at, ancestors | {obj_id})
            elif 'Frame' in slot['cap']:
                cap = slot['cap']['Frame']
                if root == owner_roots[0] and cap['object'] in msa_chosen.values():
                    raise AssertionError('WLAN MSA frame mapped into owner VSpace')
                frame = objects[cap['object']]['object']['Frame']
                if root == owner_roots[0]:
                    for _, _, size, virtual, mapped in spans():
                        if not mapped:
                            assert not overlap(at, 1 << frame['size_bits'], virtual, size), 'mapped virtual guard'
                if cap['object'] in root_refs:
                    root_refs[cap['object']].append((root, at, cap, level))
    for i, obj in enumerate(objects):
        if isinstance(obj['object'], dict) and obj['object'].get('PageTable', {}).get('is_root'):
            walk(i, i, 0, 0, set())
    for base, i in chosen.items():
        name, virtual, mapped = expected[base]
        assert len(refs[i]) == int(mapped), 'guard authority or mapped frame alias: ' + name
        assert len(root_refs[i]) == int(mapped), 'pool mapping unreachable/aliased'
        if not mapped:
            continue
        root, at, cap, level = root_refs[i][0]
        assert root == owner_roots[0] and at == virtual and level == 3, 'wrong actual pool VSpace/address/page size'
        assert cap['rights']['read'] and cap['rights']['write'] and not cap['executable'] and not cap['cached'], 'pool rights/cache'
    return {'reserved_pages': len(chosen), 'unmapped_guard_pages': 6, 'mapped_bytes': 0x701000,
            'wlan_msa_reserved_pages': len(msa_chosen), 'wlan_msa_unmapped': True}


def validate_report(spec, report):
    # CapDL leaves dynamically allocated paddr fields null; the generated
    # report supplies actual addresses for those frames and all other objects.
    rows = re.findall(r"^\s*(\d+) - [^:]+: '([^']+)' @ (0x[0-9a-fA-F]+)$", report, re.M)
    assert rows, 'missing generated allocation report'
    expected = {base+at: name for name, base, size, _, _ in spans() for at in range(0, size, 4096)}
    msa_expected = set(range(WLAN_MSA[0], WLAN_MSA[0]+WLAN_MSA[1], 4096))
    seen, ids = set(), set()
    msa_seen = set()
    for raw_id, name, raw_base in rows:
        i, base = int(raw_id), int(raw_base, 16)
        assert i not in ids, 'duplicate report object'
        ids.add(i)
        obj = spec['objects'][i]
        assert obj['name'] == name, 'stale report object identity'
        kind, value = next(iter(obj['object'].items())) if isinstance(obj['object'], dict) else (obj['object'], {})
        if kind == 'Frame':
            size = 1 << value['size_bits']
            assert value['paddr'] is None or value['paddr'] == base, 'report physical mismatch'
        elif kind == 'CNode':
            size = 1 << (value['size_bits'] + 5)  # AArch64 seL4 slot bits
        elif kind == 'SchedContext':
            size = 1 << value['size_bits']
        else:
            size = 4096  # conservative whole containing page
        if kind == 'Frame' and overlap(base, size, WLAN_MSA[0], WLAN_MSA[1]):
            assert size == 4096 and base in msa_expected and name.startswith('frame_mr_wlan_msa_pool_') and base not in msa_seen, 'wrong WLAN MSA allocation frame'
            msa_seen.add(base)
        elif base in expected and kind == 'Frame':
            assert size == 4096 and name.startswith('frame_mr_' + expected[base] + '_') and base not in seen, 'wrong reserved frame'
            seen.add(base)
        else:
            validate_exclusions([(name, base, size, False)])
    assert seen == expected.keys(), 'allocation report missing pool/guard frames'
    assert msa_seen == msa_expected, 'allocation report missing WLAN MSA pages'
    frames = {i for i, o in enumerate(spec['objects']) if isinstance(o['object'], dict) and 'Frame' in o['object']}
    assert frames <= ids, 'allocation report missing dynamic frames'
    return {'actual_allocations_checked': len(rows), 'wlan_msa_pages_reported': len(msa_seen)}


def validate_exclusions(regions, include_msa=True):
    for name, base, size, _, _ in spans():
        for excluded, start, length, _ in regions:
            assert 0 <= start < 1 << 64 and 0 < length and start+length <= 1 << 64, 'invalid exclusion range'
            assert not overlap(base, size, start, length), 'pool overlaps reservation: ' + excluded
    if include_msa:
        for excluded, start, length, _ in regions:
            assert 0 <= start < 1 << 64 and 0 < length and start+length <= 1 << 64, 'invalid exclusion range'
            assert not overlap(WLAN_MSA[0], WLAN_MSA[1], start, length), 'WLAN MSA overlaps reservation: ' + excluded


def elf_segments(path):
    b = Path(path).read_bytes()
    assert b[:6] == b'\x7fELF\x02\x01', 'ELF64 LE required'
    off = struct.unpack_from('<Q', b, 32)[0]
    stride, count = struct.unpack_from('<HH', b, 54)
    return [ph for i in range(count) if (ph := struct.unpack_from('<IIQQQQQQ', b, off+i*stride))[0] == 1]


def validate_image(image, loader_elf, profile):
    segments = elf_segments(loader_elf)
    b = Path(image).read_bytes()
    base = min(ph[3] for ph in segments)
    assert base == int(profile['loader_link_address'], 0)
    header = max(ph[3]+ph[6] for ph in segments)-base
    magic, size, _, initial, initial_end, _, _, count = struct.unpack_from('<8Q', b, header)
    assert magic == 0x5e14dead14de5ead and size+header == len(b) and 0 < count <= 64, 'loader header'
    source = int(profile['source_image_base'], 0)
    ranges = [('whole loader', base, len(b), False), ('wrapped source', source, len(b)+4096, False),
              ('initial task', initial, initial_end-initial, False)]
    data = header+64+count*32
    for i in range(count):
        target, length, offset, kind = struct.unpack_from('<4Q', b, header+64+i*32)
        assert kind == 1 and data+offset+length <= len(b), 'loader copy bounds'
        ranges.append(('actual copy target', target, length, False))
    validate_exclusions(ranges)
    return {'loader_and_copy_targets_disjoint': True, 'loader_copy_targets': count}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--system', type=Path, default=ROOT/'platforms/sunfish/modem-owner.system')
    p.add_argument('--capdl', type=Path)
    p.add_argument('--report', type=Path)
    p.add_argument('--image', type=Path)
    p.add_argument('--loader-elf', type=Path)
    p.add_argument('--profile', type=Path)
    p.add_argument('--dt', action='append', type=Path, default=[])
    a = p.parse_args()
    validate_manifest(a.system)
    result = {}
    if a.capdl:
        spec = json.loads(a.capdl.read_text())
        result.update(validate_capdl(spec))
        assert a.report, 'actual allocation report required'
        result.update(validate_report(spec, a.report.read_text()))
    if a.image:
        assert a.loader_elf and a.profile, 'loader ELF and source profile required'
        result.update(validate_image(a.image, a.loader_elf, json.loads(a.profile.read_text())))
    if a.dt:
        s = importlib.util.spec_from_file_location('memory_audit', Path(__file__).with_name('audit-capdl-memory.py'))
        audit = importlib.util.module_from_spec(s); s.loader.exec_module(audit)
        for dt in a.dt:
            regions, _ = audit.read_regions(dt)
            validate_exclusions(regions, include_msa=False)
            msa = [r for r in regions if r[1:3] == WLAN_MSA]
            assert len(msa) == 1 and msa[0][3], 'cached DT must reserve exact no-map WLAN MSA range'
            # Both cached kernel/live DTs describe a RAM bank covering the
            # whole fixed profile. No Linux dynamic allocation is replayed.
            cells = audit.read_cells(dt, '/memory', 'reg')
            assert len(cells) % 4 == 0
            ram = [(cells[i] << 32 | cells[i+1], cells[i+2] << 32 | cells[i+3]) for i in range(0, len(cells), 4)]
            fixed = [(base, size) for _, base, size, _, _ in spans()] + [WLAN_MSA]
            for base, size in fixed:
                assert any(base >= start and base+size <= start+length for start, length in ram), 'pool outside RAM bank'
    print(json.dumps({'pool_reservation_validation': 'passed', **result}))


if __name__ == '__main__':
    main()
