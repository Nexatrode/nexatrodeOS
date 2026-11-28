#!/usr/bin/env python3
"""Reject authority and erasure omissions in the actual four-app assembly."""
import argparse,copy,json,sys,xml.etree.ElementTree as ET
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'experiments/desktop-apps'))
import maps,session
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);a=p.parse_args()
    stage=a.guest.resolve();assert stage.is_relative_to(ROOT/'build/desktop-apps') and (stage/'.disposable-build').is_file()
    system=ET.parse(stage/'desktop-apps.system').getroot();layout=json.loads((stage/'session-layout.json').read_text())
    aliases={k:tuple(v) for k,v in layout['private_shared'].items()};ranges=list(map(tuple,layout['ranges']));boot=list(map(tuple,layout['boot_ranges']))
    def validate(s,ar=aliases,rr=ranges):maps.validate(s);session.validate(s,ar,rr,boot,maps=True)
    def domain(s,n):return next(d for d in s.iter('protection_domain') if d.get('name')==n)
    validate(system);rejected=[]
    for name,mutate in (
        ('wallet reads app key',lambda s:ET.SubElement(domain(s,'wallet_ui'),'map',mr='map_session_key',perms='r')),
        ('ciphertext driver reads app key',lambda s:ET.SubElement(domain(s,'map_store'),'map',mr='map_session_key',perms='r')),
        ('renderer reads root enrollment',lambda s:ET.SubElement(domain(s,'offline_map'),'map',mr='device_lock_transfer',perms='r')),
        ('renderer reads disk registers',lambda s:ET.SubElement(domain(s,'offline_map'),'map',mr='map_disk_regs',perms='rw')),
        ('wrong managed child id',lambda s:domain(s,'offline_map').set('id','63')),
        ('GPU writes Maps surface',lambda s:domain(s,'desktop_gpu').find("map[@mr='map_frame']").set('perms','rw')),
    ):
        bad=copy.deepcopy(system);mutate(bad)
        try:validate(bad)
        except AssertionError:rejected.append(name)
        else:raise AssertionError('accepted '+name)
    key=aliases['map_session_key']
    try:validate(system,rr=[r for r in ranges if r!=key])
    except AssertionError:rejected.append('key delivery page omitted from erasure')
    else:raise AssertionError('key delivery omission accepted')
    assert 'map_disk_regs' not in aliases and {'offline_map','map_store'}<=layout['children'].keys()
    result=dict(passed=True,rejected=rejected,managed_children=len(layout['children']),hardware_mmio_excluded_from_byte_scrub=True)
    (stage/'session-contract-validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
if __name__=='__main__':main()
