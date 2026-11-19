#!/usr/bin/env python3
"""Validate the native owner's fixed capability/mapping/IRQ profile offline."""
from pathlib import Path
import sys
import importlib.util
import json
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]


def validate(system, include_ufs=False):
    tree = ET.parse(system).getroot() if isinstance(system, (str, Path)) else system
    pool_spec = importlib.util.spec_from_file_location('modem_pools', Path(__file__).with_name('validate-modem-pools.py'))
    pools = importlib.util.module_from_spec(pool_spec); pool_spec.loader.exec_module(pools)
    pools.validate_manifest(tree)
    pds = tree.findall('protection_domain')
    assert len(pds) == 1 and pds[0].get('name') == 'modem_image_probe', 'exclusive owner PD required'
    pd = pds[0]
    assert int(pd.get('stack_size'), 0) >= 0x100000, 'owner stack must cover native attach call chain'
    regions = {mr.get('name'): mr for mr in tree.findall('memory_region')}
    msa = regions.get('wlan_msa_pool')
    assert msa is not None, 'fixed WLAN MSA reservation required'
    assert (int(msa.get('phys_addr'), 0), int(msa.get('size'), 0), int(msa.get('page_size'), 0)) == (
        0x98d00000, 0x200000, 0x1000
    ), 'fixed WLAN MSA reservation'
    assert not tree.findall(".//map[@mr='wlan_msa_pool']"), 'WLAN MSA must remain unmapped before SCM handoff'
    expected = {
        'smem_snapshot_low': (0x86000000, 0x44000000, 0x11d000, 'r'),
        'smem_ap_page': (0x8611d000, 0x4411d000, 0x1000, 'rw'),
        'glink_prepare_tail': (0x8611e000, 0x4411e000, 0x4000, 'rw'),
        'smem_snapshot_high': (0x86122000, 0x44122000, 0xde000, 'r'),
        'tcsr_smem_lock': (0x01f43000, 0x40006000, 0x1000, 'rw'),
        'aop_doorbell': (0x17c00000, 0x40005000, 0x1000, 'rw'),
    }
    ufs_expected = {
        'ufs_hci_mmio': (0x01d84000, 0x64000000, 0x3000, 'rw'),
        'ufs_phy_mmio': (0x01d87000, 0x64003000, 0x1000, 'rw'),
        'ufs_ice_mmio': (0x01d90000, 0x64004000, 0x8000, 'rw'),
        'ufs_smmu_tables': (None, 0x66000000, 0xb000, 'rw'),
    }
    dma_region = regions.get('ufs_dma_pool')
    if include_ufs:
        expected.update(ufs_expected)
        assert dma_region is not None, 'candidate must allocate its UFS backing pool'
        assert dma_region.get('phys_addr') is None, 'UFS backing pool must use build-time physical allocation'
        assert (int(dma_region.get('size'), 0), int(dma_region.get('page_size'), 0)) == (
            0x1000000, 0x1000
        ), 'UFS backing pool size and granularity'
        expected['ufs_dma_pool'] = (None, 0x65000000, 0x1000000, 'rw')
        dma_maps = pd.findall("map[@mr='ufs_dma_pool']")
        assert len(dma_maps) == 1 and dma_maps[0].get('cached') == 'false' and \
            dma_maps[0].get('setvar_vaddr') == 'ufs_dma_pool', 'single uncached UFS backing-pool mapping required'
        dma_paddr = pd.findall("setvar[@symbol='ufs_dma_pool_paddr']")
        assert len(dma_paddr) == 1 and dma_paddr[0].get('region_paddr') == 'ufs_dma_pool', \
            'UFS physical-base symbol must come from Microkit allocation'
        table_region = regions['ufs_smmu_tables']
        assert table_region.get('phys_addr') is None and (
            int(table_region.get('size'), 0), int(table_region.get('page_size'), 0)
        ) == (0xb000, 0x1000), 'UFS LPAE tables need eleven build-allocated 4 KiB pages'
        table_maps = pd.findall("map[@mr='ufs_smmu_tables']")
        assert len(table_maps) == 1 and table_maps[0].get('cached') == 'false' and \
            table_maps[0].get('setvar_vaddr') == 'ufs_smmu_tables', \
            'single uncached UFS LPAE table mapping required'
        table_paddr = pd.findall("setvar[@symbol='ufs_smmu_tables_paddr']")
        assert len(table_paddr) == 1 and table_paddr[0].get('region_paddr') == 'ufs_smmu_tables', \
            'UFS LPAE physical-base symbol must come from Microkit allocation'
    else:
        assert not (set(ufs_expected) & set(regions)), 'active owner must not acquire candidate UFS resources'
        assert dma_region is None and not tree.findall(".//setvar[@symbol='ufs_dma_pool_paddr']"), \
            'active owner must not acquire candidate UFS backing-pool resources'
    found = set()
    mapped = []
    for mapping in pd.findall('map'):
        name = mapping.get('mr')
        mr = regions[name]
        size = int(mr.get('size'), 0)
        virtual = int(mapping.get('vaddr'), 0)
        for start, end in mapped:
            assert virtual >= end or start >= virtual + size, 'virtual alias'
        mapped.append((virtual, virtual + size))
        physical = int(mr.get('phys_addr'), 0) if mr.get('phys_addr') else None
        if name in expected:
            assert name not in found, 'duplicate critical mapping'
            found.add(name)
            paddr, vaddr, length, rights = expected[name]
            assert (physical, virtual, size, mapping.get('perms')) == (paddr, vaddr, length, rights), name
            assert mapping.get('cached') == 'false', f'{name}: cached mapping'
        elif physical is not None:
            for paddr, _, length, _ in expected.values():
                if paddr is not None:
                    assert physical >= paddr + length or paddr >= physical + size, 'physical alias of owner mapping'
        if physical is not None:
            assert physical + size <= 0x17a00000 or physical >= 0x17b00000, 'GIC mapping forbidden'
    assert found == set(expected), 'missing owner mapping'
    irqs = [(int(irq.get('id')), int(irq.get('irq')), irq.get('trigger')) for irq in pd.findall('irq')]
    expected_irqs = [(0, 481, 'edge'), (1, 483, 'edge')]
    if include_ufs:
        expected_irqs.append((4, 297, 'level'))  # GIC SPI 265 + the 32 SGI/PPI IDs.
    assert sorted(irqs) == sorted(expected_irqs), 'owner IRQ route'
    channels = tree.findall('channel')
    assert len(channels) == 1, 'one local continuation channel required'
    ends = channels[0].findall('end')
    assert [(end.get('pd'), end.get('id'), end.get('notify'), end.get('pp')) for end in ends] == [
        ('modem_image_probe', '2', 'true', 'false'), ('modem_image_probe', '3', 'false', 'false')], 'local continuation route'


def validate_capdl(spec, include_ufs=False):
    objects = spec['objects']
    owners = [obj['object']['CNode'] for obj in objects if obj.get('name') == 'cnode_modem_image_probe']
    assert len(owners) == 1, 'owner CNode'
    slots = {entry['slot']: entry['cap'] for entry in owners[0]['slots']}
    msa_frames = {
        i for i, obj in enumerate(objects)
        if isinstance(obj.get('object'), dict)
        and (frame := obj['object'].get('Frame')) is not None
        and frame.get('paddr') is not None
        and 0x98d00000 <= frame['paddr'] < 0x98f00000
    }
    assert not any(
        cap.get('Frame', {}).get('object') in msa_frames for cap in slots.values()
    ), 'WLAN MSA frame capability exposed to owner'
    roots = [
        i for i, obj in enumerate(objects)
        if obj.get('name') == 'pgd_modem_image_probe'
        and isinstance(obj.get('object'), dict)
        and obj['object'].get('PageTable', {}).get('is_root')
    ]
    assert len(roots) == 1, 'owner root page table'

    def maps_msa(object_id, seen):
        assert object_id not in seen, 'page-table cycle'
        seen.add(object_id)
        table = objects[object_id]['object']['PageTable']
        for entry in table.get('slots', []):
            cap = entry.get('cap', {})
            if cap.get('Frame', {}).get('object') in msa_frames:
                return True
            child = cap.get('PageTable', {}).get('object')
            if child is not None and maps_msa(child, seen):
                return True
        return False

    assert not maps_msa(roots[0], set()), 'WLAN MSA frame mapped in owner page table'
    incoming = slots[1]['Notification']
    continuation = slots[12]['Notification']  # output channel2 at base10
    assert continuation['object'] == incoming['object'] and continuation['badge'] == 8, 'continuation badge/object'
    assert continuation['rights']['write'] and not continuation['rights']['read'], 'continuation rights'
    routes = [(0, 481, 1), (1, 483, 1)]
    if include_ufs:
        routes.append((4, 297, 0))
    assert sorted(entry['irq'] for entry in spec['irqs']) == sorted(route[1] for route in routes), 'IRQ capabilities'
    for channel_id, irq, trigger in routes:
        handler = next(entry['handler'] for entry in spec['irqs'] if entry['irq'] == irq)
        assert slots[138 + channel_id]['ArmIrqHandler']['object'] == handler, 'IRQ channel slot'
        description = objects[handler]['object']['ArmIrq']
        notice = description['slots'][0]['cap']['Notification']
        assert notice['object'] == incoming['object'] and notice['badge'] == 1 << channel_id, 'IRQ notification badge'
        assert description['extra']['trigger'] == trigger, 'IRQ trigger'
    expected_handler_slots = {138, 139} | ({142} if include_ufs else set())
    assert {slot for slot, cap in slots.items() if 'ArmIrqHandler' in cap} == expected_handler_slots, 'extra IRQ authority'


if __name__ == '__main__':
    args = sys.argv[1:]
    include_ufs = '--ufs-profile' in args
    if include_ufs:
        args.remove('--ufs-profile')
    capdl_path = None
    if '--capdl' in args:
        capdl_index = args.index('--capdl')
        capdl_path = Path(args[capdl_index + 1])
        del args[capdl_index:capdl_index + 2]
    system_path = Path(args[0]) if args else ROOT / (
        'platforms/sunfish/modem-owner-ufs-profile.system' if include_ufs else 'platforms/sunfish/modem-owner.system'
    )
    validate(system_path, include_ufs=include_ufs)
    if capdl_path is not None:
        validate_capdl(json.loads(capdl_path.read_text()), include_ufs=include_ufs)
    profile = 'candidate UFS ' if include_ufs else ''
    print(f'{profile}owner mapping permissions, aliases, IRQ routes and continuation capability passed.')
