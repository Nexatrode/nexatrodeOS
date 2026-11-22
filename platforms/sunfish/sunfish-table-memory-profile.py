#!/usr/bin/env python3
"""Audit final CapDL mappings for the eight-table Normal-NC experiment.

This supplements the runtime capability audit. It does not audit the kernel,
initializer's temporary mappings, cache maintenance, or hardware behavior.
"""
from pathlib import Path
import xml.etree.ElementTree as ET


TABLE_VADDRS = {
    'smmu_root': 0x71001000, 'smmu_l2': 0x71003000,
    'smmu_l3': 0x71005000, 'smmu_upper': 0x71007000,
    'smmu_l2_peer': 0x71009000, 'smmu_l3_peer': 0x7100b000,
    'smmu_l3_shared': 0x7100d000, 'smmu_l3_receive': 0x7100f000,
}
PD = 'native_startup_preflight'


def _require(condition, message):
    if not condition:
        raise ValueError('table memory profile: ' + message)


def _integer(value, message):
    _require(type(value) is int and value >= 0, message)
    return value


def _xml_integer(value, message):
    try:
        result = int(value, 0)
    except (TypeError, ValueError):
        raise ValueError('table memory profile: ' + message) from None
    _require(result >= 0, message)
    return result


def _xml_bool(node, key, default=False):
    value = node.get(key)
    if value is None:
        return default
    _require(value in ('true', 'false'), 'invalid XML boolean ' + key)
    return value == 'true'


def _bool(mapping, key, default=None):
    value = mapping.get(key, default)
    _require(type(value) is bool, 'invalid CapDL boolean ' + key)
    return value


def _slots(body):
    slots = body.get('slots', [])
    _require(isinstance(slots, list), 'malformed slots')
    seen = set()
    for entry in slots:
        _require(isinstance(entry, dict), 'malformed slot entry')
        slot = _integer(entry.get('slot'), 'invalid slot number')
        _require(slot not in seen, 'duplicate slot number')
        seen.add(slot)
        cap = entry.get('cap')
        _require(isinstance(cap, dict) and len(cap) == 1, 'malformed capability')
        yield slot, cap


def audit(systemPath, model, experimental=False):
    """Raise ValueError on mismatch; return counts and selected object IDs.

    Missing normal_nc fields mean false for compatibility with baseline CapDL.
    Booleans that are present must be actual JSON booleans or XML true/false.
    """
    _require(type(experimental) is bool, 'experimental must be boolean')
    try:
        root = ET.parse(Path(systemPath)).getroot()
    except (OSError, ET.ParseError) as error:
        raise ValueError('table memory profile: invalid system XML') from error
    _require(root.tag == 'system', 'expected system XML')
    regions = {}
    for region in root.iter('memory_region'):
        name = region.get('name')
        _require(bool(name) and name not in regions, 'duplicate/missing memory-region name')
        regions[name] = region
    pds = {}
    for pd in root.iter('protection_domain'):
        name = pd.get('name')
        _require(bool(name) and name not in pds, 'duplicate/missing protection-domain name')
        pds[name] = pd
    _require(PD in pds, 'missing native protection domain')
    parents = {child: parent for parent in root.iter() for child in parent}
    maps = list(root.iter('map'))
    for node in root.iter():
        normal = _xml_bool(node, 'normal_nc')
        _require(not normal or (experimental and node.tag == 'map'
                                and node.get('mr') in TABLE_VADDRS),
                 'Normal-NC XML attribute outside experimental table maps')
    for mapping in maps:
        _require(mapping.get('mr') in regions, 'mapping references unknown memory region')
        normal = _xml_bool(mapping, 'normal_nc')
        _xml_bool(mapping, 'cached', True)
        if normal:
            _require(experimental and mapping.get('mr') in TABLE_VADDRS,
                     'Normal-NC XML mapping outside experimental tables')
    for name, address in TABLE_VADDRS.items():
        _require(name in regions, 'missing table RAM region ' + name)
        region = regions[name]
        _require(parents.get(region) is root, 'table RAM declaration must be top-level: ' + name)
        _require('phys_addr' not in region.attrib, 'table must use allocated RAM: ' + name)
        _require(_xml_integer(region.get('size'), 'invalid table size') == 4096
                 and _xml_integer(region.get('page_size', '0x1000'), 'invalid table page size') == 4096,
                 'table must be one 4KiB RAM frame: ' + name)
        selected = [mapping for mapping in maps if mapping.get('mr') == name]
        _require(len(selected) == 1, 'table XML mapping multiplicity: ' + name)
        mapping = selected[0]
        _require(parents.get(mapping) is pds[PD], 'table mapped by wrong protection domain: ' + name)
        _require(_xml_integer(mapping.get('vaddr'), 'invalid table virtual address') == address,
                 'table virtual address changed: ' + name)
        _require(mapping.get('perms') == 'rw' and not _xml_bool(mapping, 'cached', True),
                 'table XML must remain uncached RW/NX: ' + name)
        _require(_xml_bool(mapping, 'normal_nc') is experimental,
                 'table XML Normal-NC mode mismatch: ' + name)
        for other in pds[PD].findall('map'):
            if other is mapping:
                continue
            start = _xml_integer(other.get('vaddr'), 'invalid native mapping address')
            size = _xml_integer(regions[other.get('mr')].get('size'), 'invalid native mapping size')
            _require(start + size <= address or address + 4096 <= start,
                     'another XML map overlaps table frame: ' + name)

    _require(isinstance(model, dict) and isinstance(model.get('objects'), list), 'missing CapDL objects')
    normal_nodes = []
    pending = [model]
    while pending:
        node = pending.pop()
        if isinstance(node, dict):
            if 'normal_nc' in node and _bool(node, 'normal_nc'):
                _require(experimental, 'baseline forbids all Normal-NC CapDL attributes')
                normal_nodes.append(node)
            pending.extend(node.values())
        elif isinstance(node, list):
            pending.extend(node)
    objects = model['objects']
    names = {}
    bodies = []
    for index, obj in enumerate(objects):
        _require(isinstance(obj, dict), 'malformed CapDL object')
        name = obj.get('name')
        _require(isinstance(name, str) and name and name not in names, 'duplicate/missing CapDL object name')
        names[name] = index
        definition = obj.get('object')
        if isinstance(definition, str):
            bodies.append((definition, {}))
        else:
            _require(isinstance(definition, dict) and len(definition) == 1, 'malformed object definition')
            kind, body = next(iter(definition.items()))
            _require(isinstance(body, dict), 'malformed object body')
            bodies.append((kind, body))

    frames = {}
    for name in TABLE_VADDRS:
        prefix = 'frame_mr_' + name + '_'
        chosen = [i for i, obj in enumerate(objects) if obj['name'].startswith(prefix)
                  and obj['name'][len(prefix):].isascii() and obj['name'][len(prefix):].isdigit()]
        _require(len(chosen) == 1 and objects[chosen[0]]['name'] == prefix + '000000000',
                 'table frame identity/multiplicity: ' + name)
        index = chosen[0]
        kind, frame = bodies[index]
        _require(kind == 'Frame' and type(frame.get('size_bits')) is int and frame['size_bits'] == 12,
                 'table final frame size: ' + name)
        _require(frame.get('init') == {'entries': []}, 'table frame must have no initialization entries: ' + name)
        physical = _integer(frame.get('paddr'), 'table physical address unavailable')
        _require(physical % 4096 == 0, 'unaligned table physical frame')
        frames[name] = index
    chosen_ids = set(frames.values())
    for name, index in frames.items():
        physical = bodies[index][1]['paddr']
        for other, (kind, frame) in enumerate(bodies):
            if kind != 'Frame' or other == index or frame.get('paddr') is None:
                continue
            base = _integer(frame.get('paddr'), 'invalid frame physical address')
            bits = _integer(frame.get('size_bits'), 'invalid frame size')
            _require(bits <= 64, 'invalid frame size')
            _require(base + (1 << bits) <= physical or physical + 4096 <= base,
                     'physical frame alias for ' + name)

    frame_caps = {index: [] for index in chosen_ids}
    allowed_normal_nodes = set()
    for owner, (_, body) in enumerate(bodies):
        for slot, cap in _slots(body):
            if 'Frame' not in cap:
                continue
            frame = cap['Frame']
            _require(isinstance(frame, dict), 'malformed frame capability')
            target = _integer(frame.get('object'), 'invalid frame capability object')
            _require(target < len(objects) and bodies[target][0] == 'Frame', 'frame cap target is not a frame')
            normal = _bool(frame, 'normal_nc', False)
            cached = _bool(frame, 'cached')
            executable = _bool(frame, 'executable')
            if normal:
                _require(experimental and target in chosen_ids, 'Normal-NC capability outside experimental tables')
            if target in chosen_ids:
                rights = frame.get('rights')
                _require(isinstance(rights, dict) and set(rights) == {'read', 'write', 'grant', 'grant_reply'},
                         'table capability rights shape')
                values = {key: _bool(rights, key) for key in rights}
                _require(values == dict(read=True, write=True, grant=False, grant_reply=False)
                         and not cached and not executable and normal is experimental,
                         'table capability memory type/rights changed')
                frame_caps[target].append((owner, slot))
                if normal:
                    allowed_normal_nodes.add(id(frame))
    _require(all(id(node) in allowed_normal_nodes for node in normal_nodes),
             'Normal-NC attribute outside final table frame capabilities')
    _require(all(len(caps) == 1 for caps in frame_caps.values()), 'table frame capability multiplicity')

    tcb_index = names.get('tcb_' + PD)
    _require(tcb_index is not None and bodies[tcb_index][0] == 'Tcb', 'native TCB missing')
    tcb_slots = dict(_slots(bodies[tcb_index][1]))
    root_cap = tcb_slots.get(1, {}).get('PageTable', {})
    _require(isinstance(root_cap, dict), 'native TCB root capability missing')
    native_root = _integer(root_cap.get('object'), 'native TCB root capability missing')
    _require(native_root < len(objects) and objects[native_root]['name'] == 'pgd_' + PD,
             'native TCB root identity changed')
    # A second TCB sharing this VSpace would create another PD's mapping even
    # though the leaf Frame capability appears only once in the JSON tree.
    for index, (kind, body) in enumerate(bodies):
        if kind == 'Tcb' and index != tcb_index:
            cap = dict(_slots(body)).get(1, {}).get('PageTable', {})
            _require(not isinstance(cap, dict) or cap.get('object') != native_root,
                     'another TCB aliases the native address space')

    occurrences = {index: [] for index in chosen_ids}

    def walk(index, level, base, ancestors, origin):
        _require(index < len(objects) and index not in ancestors, 'page-table target/cycle')
        kind, table = bodies[index]
        _require(kind == 'PageTable' and type(table.get('level')) is int and table['level'] == level,
                 'page-table level mismatch')
        _require(_bool(table, 'is_root') is (level == 0) and not _bool(table, 'x86_ept'),
                 'unexpected page-table root/architecture')
        shift = 12 + 9 * (3 - level)
        for slot, cap in _slots(table):
            _require(slot < 512, 'page-table slot exceeds AArch64 geometry')
            address = base | (slot << shift)
            if 'PageTable' in cap:
                _require(level < 3 and isinstance(cap['PageTable'], dict), 'invalid subtable mapping')
                target = _integer(cap['PageTable'].get('object'), 'invalid subtable target')
                walk(target, level + 1, address, ancestors | {index}, origin)
            else:
                _require('Frame' in cap and level > 0, 'unexpected page-table capability')
                target = cap['Frame']['object']
                _require(bodies[target][1].get('size_bits') == shift, 'frame mapping/page-table size mismatch')
                if target in chosen_ids:
                    occurrences[target].append((origin, address))

    roots = [i for i, (kind, body) in enumerate(bodies) if kind == 'PageTable' and body.get('is_root') is True]
    _require(native_root in roots, 'native VSpace is not a root')
    for origin in roots:
        walk(origin, 0, 0, set(), origin)
    for name, index in frames.items():
        _require(occurrences[index] == [(native_root, TABLE_VADDRS[name])],
                 'table mapping unreachable, aliased, or in wrong VSpace/address: ' + name)
    return dict(experimental=experimental, table_frames=8,
                normal_nc_frames=8 if experimental else 0, table_frame_ids=frames)


def negative_controls(systemPath, model, experimental=False):
    """Run isolated negative mutations against a valid pair; return labels.

    Inputs are never mutated. The caller should also run its complete existing
    capability mutation suite; these controls cover only this helper's policy.
    """
    import copy
    import tempfile

    result = audit(systemPath, model, experimental)
    original = ET.parse(systemPath).getroot()
    labels = []
    with tempfile.TemporaryDirectory(prefix='sunfish-table-memory-') as directory:
        path = Path(directory) / 'changed.system'

        def reject(label, xml, changed):
            ET.ElementTree(xml).write(path)
            try:
                audit(path, changed, experimental)
            except ValueError:
                labels.append(label)
            else:
                raise ValueError('table memory profile mutation accepted: ' + label)

        for name in TABLE_VADDRS:
            xml = copy.deepcopy(original)
            xml.find('.//map[@mr="' + name + '"]').set('normal_nc', 'false' if experimental else 'true')
            reject(name + ' XML mode', xml, model)
            changed = copy.deepcopy(model)
            target = result['table_frame_ids'][name]
            for obj in changed['objects']:
                body = obj['object']
                if isinstance(body, dict):
                    for data in body.values():
                        if isinstance(data, dict):
                            for entry in data.get('slots', []):
                                cap = entry['cap'].get('Frame', {})
                                if cap.get('object') == target:
                                    cap['normal_nc'] = not experimental
            reject(name + ' CapDL mode', original, changed)

        xml = copy.deepcopy(original)
        ET.SubElement(xml, 'memory_region', name='forbidden_nc_ram', size='0x1000')
        ET.SubElement(xml.find('protection_domain'), 'map', mr='forbidden_nc_ram',
                      vaddr='0x72000000', cached='false', normal_nc='true')
        reject('other XML Normal-NC', xml, model)
        xml = copy.deepcopy(original)
        xml.find('.//map[@mr="smmu_root"]').set('vaddr', '0x71002000')
        reject('table XML address', xml, model)
        xml = copy.deepcopy(original)
        xml.find('.//map[@mr="smmu_root"]').set('perms', 'rwx')
        reject('table XML executable', xml, model)
        xml = copy.deepcopy(original)
        xml.append(copy.deepcopy(xml.find('memory_region[@name="smmu_root"]')))
        reject('table XML duplicate region', xml, model)
        target = result['table_frame_ids']['smmu_root']
        changed = copy.deepcopy(model)
        changed['objects'][target]['object']['Frame']['init']['entries'] = [{'unexpected': True}]
        reject('table initialized', original, changed)
        changed = copy.deepcopy(model)
        alias = copy.deepcopy(changed['objects'][target])
        alias['name'] = 'frame_mr_forbidden_alias_000000000'
        changed['objects'].append(alias)
        reject('table alias', original, changed)
        for key in ['cached', 'executable']:
            changed = copy.deepcopy(model)
            for obj in changed['objects']:
                body = obj['object']
                if isinstance(body, dict):
                    for data in body.values():
                        if isinstance(data, dict):
                            for entry in data.get('slots', []):
                                cap = entry['cap'].get('Frame', {})
                                if cap.get('object') == target:
                                    cap[key] = True
            reject('table CapDL ' + key, original, changed)
    return labels
