#!/usr/bin/env python3
"""Audit the actual one-shot MPSS PAS4/ready image, including large frames."""
from pathlib import Path
import argparse
import copy
import importlib.util
import json
import hashlib
import re
import tempfile
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
s = importlib.util.spec_from_file_location('auth_preauth_capdl', HERE / 'validate-native-mpss-preauth.py')
preauth = importlib.util.module_from_spec(s)
s.loader.exec_module(preauth)
base = preauth.base
base.PROFILE['mpss_pas_metadata'] = (None, 0x77501000, 0x3000, 'rw', False)
base.PROFILE['mpss_target'] = (0x8b000000, 0x88000000, 0x09800000, 'rw', False)
base.EXTRA_PHYSICAL_SYMBOLS += ('mpss_pas_metadata', 'mpss_target')
base.CONTIGUOUS_PRIVATE_REGIONS += ('mpss_pas_metadata',)
base.FRAME_BITS['mpss_target'] = 21
base.FRAME_BITS['reserved_display_tail'] = 21
base.UNMAPPED_RESERVED_PROFILE['reserved_display_tail'] = (0x9d600000, None, 0x200000, None, False)
base.UNMAPPED_RESERVED_PROFILE['reserved_reference_xbl'] = (0x85d00000, None, 0x100000, None, False)
base.AUDIT_ALL_FRAMES_FOR_RESERVATIONS = True
base.EXTRA_IRQS += ((483, 5, 'edge'), (298, 6, 'edge'))
base.EXTRA_GUARD_PAGES += (0x77500000, 0x77504000, 0x87fff000, 0x91800000)


def audit(system, model, report, dtbs=()):
    result = preauth.audit(system, model, report, dtbs)
    metadata_rows = re.findall(r"frame_mr_mpss_pas_metadata_\d+' @ (0x[0-9a-fA-F]+)", report)
    base.require(len(metadata_rows) == 3 and all(int(pa, 16) + 4096 <= 1 << 32 for pa in metadata_rows),
                 'closed SCM32 metadata addresses')
    memory = importlib.util.spec_from_file_location('auth_dt_memory', HERE / 'audit-capdl-memory.py')
    reader = importlib.util.module_from_spec(memory)
    memory.loader.exec_module(reader)
    # Both cached boot DTs describe the selected live 152 MiB allocation.
    # The generic allocator audit already rejects all private RAM in any
    # reservation. Only this exact fixed target is deliberately mapped here.
    for dtb in dtbs:
        regions, _ = reader.read_regions(dtb)
        hits = [(name, pa, size, no_map) for name, pa, size, no_map in regions
                if pa < 0x94800000 and 0x8b000000 < pa + size]
        base.require(len(hits) == 1 and hits[0][1:] == (0x8b000000, 0x09800000, True),
                     'exact live MPSS reservation, no overlapping co-processor range')
    result.update(
        scope='Retained IPA/RMTFS/power; one-shot MPSS PAS4 authentication and delivered Q6-ready observation. No GLINK/WLAN startup or flash.',
        mpss_start=True, glink_started=False, native_wifi_proven=False,
        mpss_target_physical='0x8b000000', mpss_target_bytes=0x09800000,
        mpss_target_frame_bits=21, mpss_target_frames=76, metadata_pages=3,
        modem_extra_irq_authority=True, added_irq_authority=True,
        mpss_smp2p_irq=483, mpss_smp2p_channel=5,
        mpss_watchdog_irq=298, mpss_watchdog_channel=6,
        added_smc_authority=False, sole_existing_smc_slot=7,
        target_access_after_auth_submission=False, metadata_access_after_init_submission=False,
        all_frame_objects_checked_for_reserved_overlap=True,
        display_tail_reserved_without_pd_cap_or_mapping=True,
        reference_xbl_reserved_without_pd_cap_or_mapping=True,
        physical_authentication_proven=False, physical_ready_interrupt_proven=False,
        production_ready=False)
    return result


def mutations(system, model, report):
    rejected = preauth.mutations(system, model, report)

    def reject(tree=None, changed=None, changed_report=None, label=''):
        nonlocal rejected
        with tempfile.NamedTemporaryFile(suffix='.system') as f:
            if tree is not None:
                tree.write(f.name)
            try:
                audit(f.name if tree is not None else system,
                      model if changed is None else changed,
                      report if changed_report is None else changed_report)
            except ValueError:
                rejected += 1
            else:
                raise ValueError('auth mutation accepted: ' + label)

    for name in ('mpss_pas_metadata', 'mpss_target'):
        for field, value in [('perms', 'r'), ('cached', 'true'), ('vaddr', '0x77500000'),
                             ('setvar_vaddr', 'wrong')]:
            tree = ET.parse(system)
            tree.find(f"protection_domain/map[@mr='{name}']").set(field, value)
            reject(tree=tree, label=name + '/' + field)
        for field, value in [('size', '0x1000'), ('phys_addr', '0x8b200000'), ('page_size', '0x4000')]:
            tree = ET.parse(system)
            tree.find(f"memory_region[@name='{name}']").set(field, value)
            reject(tree=tree, label=name + '/' + field)
        tree = ET.parse(system)
        tree.find(f"protection_domain/setvar[@symbol='{name}_paddr']").set('region_paddr', 'rmtfs_scm')
        reject(tree=tree, label=name + '/physical symbol')
        targets = [i for i, obj in enumerate(model['objects'])
                   if re.fullmatch('frame_mr_' + name + r'_\d+', obj['name'])]
        for target in {targets[0], targets[len(targets)//2], targets[-1]}:
            for field, value in [('rights', {'read': True, 'write': False}),
                                 ('cached', True), ('executable', True)]:
                changed = copy.deepcopy(model)
                for obj in changed['objects']:
                    if not isinstance(obj['object'], dict):
                        continue
                    for container in obj['object'].values():
                        if isinstance(container, dict):
                            for slot in container.get('slots', []):
                                cap = slot['cap'].get('Frame')
                                if cap and cap['object'] == target:
                                    cap[field] = value
                reject(changed=changed, label=f'{name}/{target}/{field}')
        changed = copy.deepcopy(model)
        changed['objects'][targets[0]]['object']['Frame']['size_bits'] = 12 if name == 'mpss_target' else 21
        reject(changed=changed, label=name + '/frame granule')
    for irq, channel in ((483, 5), (298, 6)):
        for field, value in [('trigger', 'level'), ('id', '4'), ('irq', '481')]:
            tree = ET.parse(system)
            tree.find(f"protection_domain/irq[@irq='{irq}']").set(field, value)
            reject(tree=tree, label=f'IRQ {irq}/{field}')
        handler = next(q['handler'] for q in model['irqs'] if q['irq'] == irq)
        for case in ('trigger', 'badge', 'destination', 'slot'):
            changed = copy.deepcopy(model)
            obj = changed['objects'][handler]['object']['ArmIrq']
            if case == 'trigger': obj['extra']['trigger'] = 0
            if case == 'badge': obj['slots'][0]['cap']['Notification']['badge'] = 1 << 4
            if case == 'destination': obj['slots'][0]['cap']['Notification']['object'] = 0
            if case == 'slot':
                node = next(o['object']['CNode'] for o in changed['objects']
                            if o['name'] == 'cnode_native_startup_preflight')
                slot = next(s for s in node['slots'] if s['slot'] == 138 + channel)
                slot['slot'] = 138 + 4
            reject(changed=changed, label=f'IRQ {irq}/{case}')
    for name in base.UNMAPPED_RESERVED_PROFILE:
        tree = ET.parse(system)
        ET.SubElement(tree.find('protection_domain'), 'map', mr=name, vaddr='0x78000000', perms='r', cached='false')
        reject(tree=tree, label=name + '/mapped reservation')
        for field, value in [('size', '0x1000'), ('phys_addr', '0x80000000')]:
            tree = ET.parse(system)
            tree.find(f"memory_region[@name='{name}']").set(field, value)
            reject(tree=tree, label=name + '/' + field)
        target = next(i for i, obj in enumerate(model['objects'])
                      if re.fullmatch('frame_mr_' + name + r'_\d+', obj['name']))
        changed = copy.deepcopy(model)
        node = next(o['object']['CNode'] for o in changed['objects']
                    if o['name'] == 'cnode_native_startup_preflight')
        node['slots'].append({'slot': 255, 'cap': {'Frame': {'object': target,
                            'rights': {'read': True, 'write': False}, 'cached': False, 'executable': False}}})
        reject(changed=changed, label=name + '/extra PD frame capability')
    elf = next((i, obj) for i, obj in enumerate(model['objects'])
               if obj['name'].startswith('frame_elf_native_startup_preflight_'))
    index, obj = elf
    pattern = rf"(\s*{index} - [^:]+: '{re.escape(obj['name'])}' @ )0x[0-9a-fA-F]+"
    changed_report, count = re.subn(pattern, r'\g<1>0x85e10000', report)
    base.require(count == 1, 'unique ELF frame allocation record')
    reject(changed_report=changed_report, label='unselected ELF frame in reserved XBL/AOP memory')
    return rejected


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--system', type=Path, default=HERE / 'native-startup-mpss-auth.system')
    p.add_argument('--capdl', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--dt', type=Path, action='append', default=[])
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    model = json.loads(a.capdl.read_text())
    report = a.report.read_text()
    paths = [a.system, a.capdl, a.report] + a.dt + [preauth.smem.rmtfs.REFERENCE]
    hashes = {str(p.resolve()): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    result = audit(a.system, model, report, a.dt)
    result['mutations_rejected'] = mutations(a.system, model, report)
    for path, expected in hashes.items():
        base.require(hashlib.sha256(Path(path).read_bytes()).hexdigest() == expected, 'audit input changed')
    result['input_sha256'] = hashes
    a.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
