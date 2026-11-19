#!/usr/bin/env python3
"""Mutation checks against an actual generated native SMMU probe CapDL model."""
import argparse
import copy
import importlib.util
import json
import re
from pathlib import Path
import tempfile
import xml.etree.ElementTree as ET


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--capdl', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--system', type=Path)
    parser.add_argument('--data-pages', type=int, choices=[1,32,37], default=1)
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    name = 'validate-smmu-root-install-probe.py' if args.data_pages == 37 else 'validate-smmu-install-probe.py'
    module = importlib.util.spec_from_file_location('audit', here / name)
    audit = importlib.util.module_from_spec(module)
    module.loader.exec_module(audit)
    system = args.system or here / 'smmu-install-probe.system'
    model, report = json.loads(args.capdl.read_text()), args.report.read_text()
    audit.audit(system, model, report, data_pages=args.data_pages)
    checked = []

    def rejected(name, change):
        candidate = copy.deepcopy(model)
        change(candidate)
        try:
            audit.audit(system, candidate, report, data_pages=args.data_pages)
        except audit.AuditError:
            checked.append(name)
        else:
            raise AssertionError('audit accepted: ' + name)

    def cap(model, name):
        index = next(i for i, o in enumerate(model['objects']) if re.fullmatch('frame_mr_' + re.escape(name) + r'_\d+', o['name']))
        for obj in model['objects']:
            if isinstance(obj['object'], dict):
                for container in obj['object'].values():
                    if isinstance(container, dict):
                        for entry in container.get('slots', []):
                            if entry['cap'].get('Frame', {}).get('object') == index:
                                return entry['cap']['Frame']
        raise AssertionError('missing frame cap')

    rejected('cached private data', lambda m: cap(m, 'smmu_data').update(cached=True))
    rejected('cached page tables', lambda m: cap(m, 'smmu_root').update(cached=True))
    rejected('executable data', lambda m: cap(m, 'smmu_data').update(executable=True))
    rejected('writable other context bank', lambda m: cap(m, 'smmu_cb_high')['rights'].update(write=True))
    rejected('wrong fault IRQ', lambda m: m['irqs'][0].update(irq=98))

    def extra_cap(model):
        node = next(o['object']['CNode'] for o in model['objects'] if o['name'] == 'cnode_smmu_install_probe')
        node['slots'].append({'slot': 200, 'cap': {'Frame': copy.deepcopy(cap(model, 'smmu_root'))}})
    rejected('extra frame alias', extra_cap)

    def smc(model):
        node = next(o['object']['CNode'] for o in model['objects'] if o['name'] == 'cnode_smmu_install_probe')
        node['slots'].append({'slot': 201, 'cap': {'ArmSmc': {}}})
    rejected('SMC authority', smc)

    with tempfile.TemporaryDirectory() as directory:
        xml = ET.parse(system)
        xml.find(".//map[@mr='smmu_cb_low']").set('perms', 'rw')
        changed = Path(directory) / 'changed.system'
        xml.write(changed)
        try:
            audit.audit(changed, model, report, data_pages=args.data_pages)
        except audit.AuditError:
            checked.append('writable XML context span')
        else:
            raise AssertionError('audit accepted changed XML rights')
    if args.data_pages in [32,37]:
        pages = [(int(i),name,int(address,16)) for i,name,address in re.findall(r"^\s*(\d+) - [^:]+: '(frame_mr_smmu_data_\d+)' @ (0x[0-9a-fA-F]+)$",report,re.M)]
        index,name,address = max(pages,key=lambda entry:entry[2])
        changed = re.sub(r"('"+re.escape(name)+r"' @ )0x[0-9a-fA-F]+", lambda m:m[1]+hex(address+8192), report, count=1)
        if changed == report: raise AssertionError('pool report mutation did not apply')
        changed_model = copy.deepcopy(model)
        changed_model['objects'][index]['object']['Frame']['paddr'] = address+8192
        try:
            audit.audit(system,changed_model,changed,data_pages=args.data_pages)
        except audit.AuditError as error:
            if 'contiguous private pool' not in str(error): raise
            checked.append('noncontiguous physical pool report')
        else: raise AssertionError('audit accepted discontiguous private pool')
    if args.data_pages == 37:
        for name in ['smmu_l2_peer','smmu_l3_peer','smmu_l3_shared']:
            rejected('cached '+name,lambda m,n=name:cap(m,n).update(cached=True))
            rejected('executable '+name,lambda m,n=name:cap(m,n).update(executable=True))
    print('Native SMMU capability mutation checks: ' + str(len(checked)) + ' passed')


if __name__ == '__main__':
    main()
