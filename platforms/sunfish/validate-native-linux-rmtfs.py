#!/usr/bin/env python3
"""Audit real native IPA/RMTFS frames, mappings, scratch and existing IRQ/SMC."""
from pathlib import Path
import argparse
import copy
import hashlib
import importlib.util
import json
import re
import tempfile
import xml.etree.ElementTree as E

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
module_spec=importlib.util.spec_from_file_location('rmtfs_receive_capdl',HERE/'validate-native-ipa-receive.py')
receive=importlib.util.module_from_spec(module_spec);module_spec.loader.exec_module(receive)
base=receive.base.base
base.PROFILE['native_rmtfs']=(0xfde01000,0x77001000,0x200000,'rw',False)
base.PROFILE['rmtfs_scm']=(None,0x77301000,0x1000,'rw',False)
base.EXTRA_PHYSICAL_SYMBOLS+=('native_rmtfs','rmtfs_scm')
base.EXTRA_GUARD_PAGES+=(0x77000000,0x77201000,0x77300000,0x77302000)
base.CONTIGUOUS_PRIVATE_REGIONS+=('rmtfs_scm',)
REFERENCE=ROOT/'.cache/sunfish-mobian-comparison/artifacts/sm7150-google-sunfish.dtb'
REFERENCE_SHA='4254865b9f6b71d4f9ff0edd76c5ffdccb3a5fb08562b8b6549c5f0f305f752d'

def audit(system,model,report,dtbs=(),reference=REFERENCE):
    reference=Path(reference)
    if hashlib.sha256(reference.read_bytes()).hexdigest()!=REFERENCE_SHA:
        raise ValueError('wrong selected Linux reference DTB')
    result=receive.audit(system,model,report,tuple(dtbs)+(reference,))
    spec=importlib.util.spec_from_file_location('rmtfs_memory',HERE/'audit-capdl-memory.py')
    memory=importlib.util.module_from_spec(spec);spec.loader.exec_module(memory)
    regions,_=memory.read_regions(reference)
    expected=[r for r in regions if r[0]=='rmtfs@fde01000']
    if len(expected)!=1 or expected[0][1:3]!=(0xfde01000,0x200000):
        raise ValueError('wrong reference RMTFS reservation')
    result.update(scope='Retained native IPA receive plus concrete pre-MPSS RMTFS reservation, private SCM64 scratch, RAM-shadow storage/TFTP and prepared local services. No MPSS/PAS4 start, WLAN start or flash.',
        native_rmtfs_pa='0xfde01000',native_rmtfs_bytes=0x200000,native_rmtfs_pages=512,
        private_scm_argument_page=True,logical_rmtfs_assignment_limit=1,
        native_cold_contract_required=True,cold_contract_software_proven=False,
        reference_dtb_sha256=REFERENCE_SHA,mpss_start=False,physical_rmtfs_assignment_proven=False,
        production_ready=False)
    return result

def mutations(system,model,report):
    # Reuse the existing full IPA/receive audit with the expanded exact profile.
    rejected=receive.base.mutations(system,model,report)
    for name,field,value in [
        ('native_rmtfs','perms','r'),('native_rmtfs','cached','true'),
        ('native_rmtfs','vaddr','0x77002000'),('native_rmtfs','setvar_vaddr','wrong'),
        ('rmtfs_scm','perms','r'),('rmtfs_scm','cached','true'),('rmtfs_scm','vaddr','0x77300000')]:
        tree=E.parse(system);tree.find(f"protection_domain/map[@mr='{name}']").set(field,value)
        with tempfile.NamedTemporaryFile(suffix='.system')as f:
            tree.write(f.name)
            try:audit(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError('RMTFS mapping mutation accepted: '+name+'/'+field)
    for name in ['native_rmtfs','rmtfs_scm']:
        tree=E.parse(system);tree.find(f"protection_domain/setvar[@symbol='{name}_paddr']").set('region_paddr','smmu_data')
        with tempfile.NamedTemporaryFile(suffix='.system')as f:
            tree.write(f.name)
            try:audit(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError('RMTFS paddr injection mutation accepted: '+name)
        targets=[i for i,o in enumerate(model['objects']) if re.fullmatch('frame_mr_'+name+r'_\d+',o['name'])]
        for nth in ([0,255,511] if name=='native_rmtfs' else [0]):
            for field,value in [('cached',True),('executable',True),('rights',{'read':True,'write':False})]:
                changed=copy.deepcopy(model);target=targets[nth]
                for obj in changed['objects']:
                    if not isinstance(obj['object'],dict):continue
                    for container in obj['object'].values():
                        if not isinstance(container,dict):continue
                        for slot in container.get('slots',[]):
                            cap=slot['cap'].get('Frame')
                            if cap and cap['object']==target:cap[field]=value
                try:audit(system,changed,report)
                except ValueError:rejected+=1
                else:raise ValueError('RMTFS actual frame mutation accepted: '+name+'/'+field)
    return rejected

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--system',type=Path,default=HERE/'native-startup-linux-rmtfs.system')
    p.add_argument('--capdl',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    p.add_argument('--dt',type=Path,action='append',default=[]);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();model=json.loads(a.capdl.read_text());report=a.report.read_text()
    result=audit(a.system,model,report,a.dt);result['mutations_rejected']=mutations(a.system,model,report)
    a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
