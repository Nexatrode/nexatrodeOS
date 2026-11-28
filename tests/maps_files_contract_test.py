#!/usr/bin/env python3
"""Reject weakened picker authority in the actual disposable desktop/mobile SDF."""
import argparse,copy,importlib.util,json,xml.etree.ElementTree as ET
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('files_picker_build',ROOT/'experiments/files-picker/build.py');picker=importlib.util.module_from_spec(spec);spec.loader.exec_module(picker)
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);a=p.parse_args();stage=a.guest.resolve()
    assert stage.is_relative_to(ROOT/'build') and (stage/'.disposable-build').is_file()
    m=json.loads((stage/'build.json').read_text());assert m['map_files'] and not m['saved_data_used']
    mobile=m.get('maps_os_unlock',False);system=ET.parse(stage/('mobile.system' if mobile else 'desktop-apps.system')).getroot()
    domain=lambda s,n:next(d for d in s.iter('protection_domain') if d.get('name')==n)
    picker.validate(system);rejected=[]
    mutations=[('Maps enumerates Files input',lambda s:ET.SubElement(domain(s,'offline_map'),'map',mr='files_input',perms='r')),
        ('Maps writes selection',lambda s:domain(s,'offline_map').find("map[@mr='files_state']").set('perms','rw')),
        ('storage reads Vault transfer',lambda s:ET.SubElement(domain(s,'map_store'),'map',mr='files_vault_transfer',perms='r')),
        ('picker omitted from lock group',lambda s:domain(s,'maps_lock_manager' if mobile else 'desktop_manager').remove(domain(s,'files_picker'))),
        ('wallet reads file grants',lambda s:ET.SubElement(domain(s,'wallet_app'),'map',mr='files_state',perms='r'))]
    for name,mutate in mutations:
        bad=copy.deepcopy(system);mutate(bad)
        try:picker.validate(bad)
        except (AssertionError,KeyError):rejected.append(name)
        else:raise AssertionError('accepted '+name)
    result=dict(passed=True,mobile=mobile,rejected=rejected,picker_in_managed_erase_group=True)
    (stage/'files-contract-validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
if __name__=='__main__':main()
