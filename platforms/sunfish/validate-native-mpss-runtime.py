#!/usr/bin/env python3
"""Audit the isolated post-ready root's actual CapDL, not a device result."""
from pathlib import Path
import argparse
import copy
import hashlib
import importlib.util
import json
import tempfile
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('runtime_auth_capdl', HERE / 'validate-native-mpss-auth.py')
auth = importlib.util.module_from_spec(spec)
spec.loader.exec_module(auth)
base = auth.base
base.EXTRA_IRQS += ((481, 4, 'edge'),)
IRQ_CHANNELS = {132: 0, 97: 1, 343: 2, 464: 3, 481: 4, 483: 5, 298: 6}
base.EXTRA_SELF_CHANNEL = (
    dict(pd='native_startup_preflight', id='7', notify='true', pp='false'),
    dict(pd='native_startup_preflight', id='8', notify='false', pp='false'),
)


def rights(read=False, write=False, grant=False, grant_reply=False):
    return dict(read=read, write=write, grant=grant, grant_reply=grant_reply)


def named(model, name):
    matches = [(i, o) for i, o in enumerate(model['objects']) if o['name'] == name]
    base.require(len(matches) == 1, 'unique object: ' + name)
    return matches[0]


def audit_runtime_stack(system, model):
    pd = ET.parse(system).getroot().find('protection_domain')
    base.require(pd is not None and pd.get('name') == 'native_startup_preflight',
                 'runtime stack owner')
    size = int(pd.get('stack_size', '0'), 0)
    base.require(size >= 0x200000 and size % 4096 == 0,
                 'runtime stack must cover nested startup frames')
    prefix = 'frame_native_startup_preflight_stack_'
    frames = [(i, o) for i, o in enumerate(model['objects'])
              if o.get('name', '').startswith(prefix)]
    count = size // 4096
    base.require(len(frames) == count, 'runtime stack frame count')
    expected = {prefix + f'{i:09d}' for i in range(count)}
    base.require({o['name'] for _, o in frames} == expected,
                 'runtime stack frame identities')
    frame_ids = {i for i, _ in frames}
    for _, obj in frames:
        base.require(obj.get('object', {}).get('Frame', {}).get('size_bits') == 12,
                     'runtime stack uses 4 KiB frames')
    pgd_id, _ = named(model, 'pgd_native_startup_preflight')
    _, tcb = named(model, 'tcb_native_startup_preflight')
    roots = [e.get('cap') for e in tcb['object']['Tcb']['slots'] if e.get('slot') == 1]
    base.require(roots == [{'PageTable': {'object': pgd_id}}],
                 'runtime TCB uses the expected page-table root')
    stack_top = tcb['object']['Tcb'].get('extra', {}).get('sp')
    base.require(isinstance(stack_top, int) and stack_top >= size and stack_top % 4096 == 0,
                 'runtime initial stack pointer')
    reachable = set()
    stack_addresses = []
    pending = [(pgd_id, 0, 0)]
    visited = set()
    shifts = (39, 30, 21, 12)
    while pending:
        node_id, level, address = pending.pop()
        if (node_id, level, address) in visited:
            continue
        visited.add((node_id, level, address))
        base.require(isinstance(node_id, int) and 0 <= node_id < len(model['objects']),
                     'valid runtime page-table object')
        node = model['objects'][node_id].get('object')
        table = node.get('PageTable') if isinstance(node, dict) else None
        base.require(isinstance(table, dict), 'runtime page-table capability target')
        reachable.add(node_id)
        for entry in table.get('slots', []):
            slot = entry.get('slot')
            base.require(isinstance(slot, int) and 0 <= slot < 512, 'valid page-table slot')
            entry_address = address | (slot << shifts[level])
            child = entry.get('cap', {}).get('PageTable')
            if isinstance(child, dict):
                base.require(level < 3, 'page-table tree depth')
                pending.append((child.get('object'), level + 1, entry_address))
            frame = entry.get('cap', {}).get('Frame')
            if isinstance(frame, dict) and frame.get('object') in frame_ids:
                base.require(level == 3, 'stack frames mapped as 4 KiB pages')
                stack_addresses.append(entry_address)
    base.require(len(stack_addresses) == count and
                 set(stack_addresses) == set(range(stack_top - size, stack_top, 4096)),
                 'runtime stack virtual range must be contiguous below SP')
    mappings = []
    for obj_id, obj in enumerate(model['objects']):
        data = obj.get('object')
        if not isinstance(data, dict):
            continue
        page_table = data.get('PageTable')
        if not isinstance(page_table, dict):
            continue
        for entry in page_table.get('slots', []):
            cap = entry.get('cap', {}).get('Frame')
            if isinstance(cap, dict) and cap.get('object') in frame_ids:
                base.require(obj.get('name', '').startswith('pt_native_startup_preflight_vaddr_'),
                             'runtime stack must map in its own protection domain')
                base.require(obj_id in reachable, 'runtime stack page table must be reachable')
                mappings.append(cap)
    base.require(len(mappings) == count and
                 {cap['object'] for cap in mappings} == frame_ids,
                 'each runtime stack frame mapped exactly once')
    for cap in mappings:
        base.require(cap.get('rights') == rights(read=True, write=True) and
                     cap.get('cached') is True and cap.get('executable') is False,
                     'runtime stack mappings must be cached RW and non-executable')


def audit(system, model, report, dtbs=()):
    audit_runtime_stack(system, model)
    result = auth.audit(system, model, report, dtbs)
    cnode_id, node = named(model, 'cnode_native_startup_preflight')
    root_cnode_id, root_node = named(model, 'cnode_native_startup_preflight_root')
    pgd_id, _ = named(model, 'pgd_native_startup_preflight')
    notification_id, _ = named(model, 'ntfn_native_startup_preflight')
    reply_id, _ = named(model, 'reply_native_startup_preflight')
    endpoint_id, _ = named(model, 'ep_fault_monitor')
    entries = node['object']['CNode']['slots']
    slots = {e['slot']: e['cap'] for e in entries}
    # One send-only self-notification; no additional ACK alias or IPC capability.
    base.require(len(entries) == len(slots) == 13 and set(slots) == {1, 2, 3, 4, 7, 17, *range(138, 145)},
                 'exact native CNode; no duplicate or additional capability')
    base.require(node['object']['CNode']['size_bits'] == 9 and root_node['object']['CNode'] == dict(
        size_bits=6, slots=[dict(slot=0, cap={'CNode': dict(object=cnode_id, guard=0, guard_size=49)})]),
        'exact two-level native cspace and complete 64-bit lookup')
    expected_notice = dict(object=notification_id, badge=0, rights=rights(read=True))
    expected_fault = dict(object=endpoint_id, badge=1, rights=rights(write=True, grant_reply=True))
    base.require(slots[1] == {'Notification': expected_notice}, 'receive-only native notification')
    base.require(slots[17] == {'Notification': dict(object=notification_id, badge=1 << 8,
                 rights=rights(write=True))}, 'exact self-wakeup target, badge and rights')
    base.require(slots[2] == {'Endpoint': expected_fault}, 'only monitor fault endpoint')
    base.require(slots[3] == {'PageTable': {'object': pgd_id}} and
                 slots[4] == {'Reply': {'object': reply_id}}, 'same native vspace and reply')
    actual_handlers = {i for i, o in enumerate(model['objects'])
                       if isinstance(o['object'], dict) and 'ArmIrq' in o['object']}
    base.require(actual_handlers == {q['handler'] for q in model['irqs']}, 'no orphan IRQ object')
    for irq, channel in IRQ_CHANNELS.items():
        handler = next(q['handler'] for q in model['irqs'] if q['irq'] == irq)
        obj = model['objects'][handler]
        base.require(obj['name'] == f'irq_{irq}_native_startup_preflight', 'actual native IRQ owner')
        base.require(slots[138 + channel] == {'ArmIrqHandler': {'object': handler}}, 'sole IRQ ACK capability')
        desc = obj['object']['ArmIrq']
        base.require(desc['extra']['target'] == 0 and len(desc['slots']) == 1,
                     'one target-CPU IRQ notification')
        notice = {'slot': 0, 'cap': {'Notification': dict(
            object=notification_id, badge=1 << channel, rights=rights(write=True))}}
        base.require(desc['slots'][0] == notice, 'exact IRQ notification authority and badge')
    _, tcb = named(model, 'tcb_native_startup_preflight')
    tcb_entries = tcb['object']['Tcb']['slots']
    tcb_slots = {e['slot']: e['cap'] for e in tcb_entries}
    base.require(len(tcb_entries) == len(tcb_slots) and set(tcb_slots) == {0, 1, 4, 5, 6, 8},
                 'exact native TCB capability set')
    base.require(tcb_slots[0] == {'CNode': dict(object=root_cnode_id, guard=0, guard_size=0)} and
                 tcb_slots[1] == {'PageTable': {'object': pgd_id}} and
                 tcb_slots[8] == {'Notification': expected_notice} and
                 tcb_slots[5] == {'Endpoint': expected_fault}, 'TCB owns audited cspace/vspace/notification')
    result.update(
        scope='Offline post-auth native GLINK/QRTR discovery capability audit only; no device result.',
        glink_transport_enabled=True, glink_irq=481, glink_channel=4,
        all_native_irq_channels=IRQ_CHANNELS, exact_native_cnode_slots=sorted(slots),
        apcs_software_write_bits=[0, 12, 14], added_smc_authority=False,
        added_memory_mapping_authority=False, physical_self_notify_authority=True, software_wakeup_send_channel=7,
        software_wakeup_receive_channel=8,
        rmtfs_owner_retained=True, rmtfs_available_before_binding=False,
        tftp_service_published=False, wlan_firmware_tftp_serving_enabled=False,
        ipa_server_endpoint_can_be_announced=True,
        host_wlfw_ce_radio_commands=False, physical_authentication_proven=False,
        physical_ready_interrupt_proven=False, physical_glink_ready_proven=False,
        native_wifi_proven=False, production_ready=False)
    return result


def mutations(system, model, report):
    inherited = auth.mutations(system, model, report)
    labels = []

    def reject(label, tree=None, changed=None):
        with tempfile.NamedTemporaryFile(suffix='.system') as f:
            if tree is not None:
                tree.write(f.name)
            try:
                audit(f.name if tree is not None else system,
                      model if changed is None else changed, report)
            except ValueError:
                labels.append(label)
            else:
                raise ValueError('runtime mutation accepted: ' + label)

    tree = ET.parse(system)
    tree.find('protection_domain').set('stack_size', '0x100000')
    reject('runtime stack too small', tree=tree)

    for field, value in [('irq', '482'), ('id', '0'), ('id', '3'), ('id', '5'), ('trigger', 'level')]:
        tree = ET.parse(system)
        tree.find("protection_domain/irq[@irq='481']").set(field, value)
        reject('GLINK XML ' + field + '=' + value, tree=tree)
    tree = ET.parse(system)
    tree.find('protection_domain').remove(tree.find("protection_domain/irq[@irq='481']"))
    reject('missing GLINK IRQ', tree=tree)
    handler = next(q['handler'] for q in model['irqs'] if q['irq'] == 481)
    cnode_id, _ = named(model, 'cnode_native_startup_preflight')
    root_cnode_id, _ = named(model, 'cnode_native_startup_preflight_root')
    tcb_id, _ = named(model, 'tcb_native_startup_preflight')
    for badge in (1, 8, 32):
        changed = copy.deepcopy(model)
        changed['objects'][handler]['object']['ArmIrq']['slots'][0]['cap']['Notification']['badge'] = badge
        reject('GLINK aliases native badge ' + str(badge), changed=changed)
    for field, value in [('read', True), ('write', False), ('grant', True), ('grant_reply', True)]:
        changed = copy.deepcopy(model)
        changed['objects'][handler]['object']['ArmIrq']['slots'][0]['cap']['Notification']['rights'][field] = value
        reject('GLINK notification rights ' + field, changed=changed)
    changed = copy.deepcopy(model)
    changed['objects'][handler]['object']['ArmIrq']['extra']['target'] = 1
    reject('wrong interrupt target CPU', changed=changed)
    changed = copy.deepcopy(model)
    changed['objects'][handler]['object']['ArmIrq']['slots'].append(copy.deepcopy(
        changed['objects'][handler]['object']['ArmIrq']['slots'][0]))
    reject('second IRQ notification capability', changed=changed)
    for case in ('extra ACK alias', 'duplicate CNode slot', 'physical self-notify', 'grant fault capability'):
        changed = copy.deepcopy(model)
        slots = changed['objects'][cnode_id]['object']['CNode']['slots']
        if case in ('extra ACK alias', 'duplicate CNode slot'):
            entry = copy.deepcopy(next(e for e in slots if e['slot'] == 142))
            entry['slot'] = 255 if case == 'extra ACK alias' else 142
            slots.append(entry)
        elif case == 'physical self-notify':
            slots[0]['cap']['Notification']['rights']['write'] = True
        else:
            next(e for e in slots if e['slot'] == 2)['cap']['Endpoint']['rights']['grant'] = True
        reject(case, changed=changed)
    for field, value in [('object', 0), ('badge', 1 << 4), ('rights', rights(read=True, write=True))]:
        changed = copy.deepcopy(model)
        slots = changed['objects'][cnode_id]['object']['CNode']['slots']
        next(e for e in slots if e['slot'] == 17)['cap']['Notification'][field] = value
        reject('self-wakeup ' + field, changed=changed)
    for slot, cap in [(0, 'CNode'), (1, 'PageTable'), (8, 'Notification')]:
        changed = copy.deepcopy(model)
        slots = changed['objects'][tcb_id]['object']['Tcb']['slots']
        next(e for e in slots if e['slot'] == slot)['cap'][cap]['object'] = 0
        reject('TCB audit bypass slot ' + str(slot), changed=changed)
    for case in ('additional root cspace capability', 'root cspace child substituted', 'root cspace guard shortened'):
        changed = copy.deepcopy(model)
        root = changed['objects'][root_cnode_id]['object']['CNode']
        if case == 'additional root cspace capability':
            entry = copy.deepcopy(root['slots'][0])
            entry['slot'] = 1
            root['slots'].append(entry)
        elif case == 'root cspace child substituted':
            root['slots'][0]['cap']['CNode']['object'] = 0
        else:
            root['slots'][0]['cap']['CNode']['guard_size'] = 48
        reject(case, changed=changed)
    return inherited, labels


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--system', type=Path, required=True)
    p.add_argument('--capdl', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--dt', type=Path, action='append', default=[])
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--tftp-observer', action='store_true', help='report the optional metadata-only QRTR observer profile')
    p.add_argument('--tftp-server-check', action='store_true', help='report the optional RAM-only server-check profile')
    a = p.parse_args()
    if a.tftp_observer and a.tftp_server_check:
        p.error('TFTP profiles are mutually exclusive')
    # Imported audit code is part of this proof, not just its input data.
    paths = [a.system, a.capdl, a.report, Path(__file__).resolve(),
             auth.preauth.smem.rmtfs.REFERENCE] + a.dt
    paths += sorted(HERE.glob('validate-native-*.py'))
    paths += [HERE / 'audit-capdl-memory.py']
    hashes = {str(q.resolve()): hashlib.sha256(q.read_bytes()).hexdigest() for q in paths}
    model = json.loads(a.capdl.read_text())
    report = a.report.read_text()
    result = audit(a.system, model, report, a.dt)
    if a.tftp_observer:
        result.pop('tftp_service_published', None)
        result.update(tftp_observer_configured=True, tftp_publication_not_proven=True,
                      scope='Offline capability audit for a metadata-only QRTR TFTP observer; no device result.')
    if a.tftp_server_check:
        result.pop('tftp_service_published', None)
        result.update(tftp_server_check_configured=True, tftp_publication_not_proven=True,
                      scope='Offline capability audit for a RAM-only TFTP server-check profile; no device result.')
    inherited, added = mutations(a.system, model, report)
    result.update(inherited_mutations_rejected=inherited, added_mutations_rejected=added,
                  mutations_rejected=inherited + len(added), device_actions=[])
    for path, expected in hashes.items():
        base.require(hashlib.sha256(Path(path).read_bytes()).hexdigest() == expected, 'audit input changed')
    result['input_sha256'] = hashes
    a.output.write_text(json.dumps(result, indent=2) + '\n')
    print('PASS: exact runtime CapDL authority, ' + str(inherited + len(added)) +
          ' rejected mutations; no device result.')


if __name__ == '__main__':
    main()
