#!/usr/bin/env python3
"""Actual CapDL audit for the retained pre-auth modem SMEM extension."""
from pathlib import Path
import argparse,copy,importlib.util,json,re,tempfile
import xml.etree.ElementTree as E
HERE=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('modem_smem_rmtfs_capdl',HERE/'validate-native-linux-rmtfs.py')
rmtfs=importlib.util.module_from_spec(s);s.loader.exec_module(rmtfs)
base=rmtfs.base
base.PROFILE['ipa_smem_after']=(0x8611e000,0x7511e000,0x1000,'r',False)
base.PROFILE['mpss_smem_payload']=(0x8611f000,0x7511f000,0x5000,'rw',False)
base.PROFILE['mpss_smem_tail']=(0x86124000,0x75124000,0xdc000,'r',False)
base.EXTRA_PHYSICAL_SYMBOLS+=('mpss_smem_payload',)

def audit(system,model,report,dtbs=()):
    result=rmtfs.audit(system,model,report,dtbs)
    result.update(scope='Retained native IPA/RMTFS plus cold locked allocation/get and zero/readback of fresh SMP2P/GLINK memory. No SMP2P activation, notification, GLINK start or MPSS/WLAN start.',
        smem_writable_metadata_pages=1,smem_writable_payload_pages=5,other_smem_pages_read_only=506,
        modem_payload_physical='0x8611f000',modem_payload_virtual='0x7511f000',modem_payload_pages=5,
        modem_extra_irq_authority=False,modem_extra_smc_authority=False,
        ipa_payload_cpu_access=False,smem_payload_access=True,subpage_ipa_tail_protection='exact native store whitelist; page permissions alone are insufficient',
        cold_native_extension_required=True,physical_modem_smem_proven=False,
        smp2p_activated=False,glink_started=False,mpss_start=False,production_ready=False)
    return result

def mutations(system,model,report):
    rejected=rmtfs.mutations(system,model,report)
    for name,field,value in [('mpss_smem_payload','perms','r'),('mpss_smem_payload','cached','true'),
        ('mpss_smem_payload','vaddr','0x7511e000'),('mpss_smem_payload','setvar_vaddr','wrong'),
        ('mpss_smem_tail','perms','rw'),('ipa_smem_after','perms','rw')]:
        tree=E.parse(system);tree.find(f"protection_domain/map[@mr='{name}']").set(field,value)
        with tempfile.NamedTemporaryFile(suffix='.system')as f:
            tree.write(f.name)
            try:audit(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError('modem XML mutation accepted: '+name+'/'+field)
    for name,field,value in [('mpss_smem_payload','phys_addr','0x8611e000'),('mpss_smem_payload','size','0x6000'),
                             ('mpss_smem_tail','phys_addr','0x86123000')]:
        tree=E.parse(system);tree.find(f"memory_region[@name='{name}']").set(field,value)
        with tempfile.NamedTemporaryFile(suffix='.system')as f:
            tree.write(f.name)
            try:audit(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError('modem physical mutation accepted: '+name+'/'+field)
    tree=E.parse(system);tree.find("protection_domain/setvar[@symbol='mpss_smem_payload_paddr']").set('region_paddr','ipa_smem_metadata')
    with tempfile.NamedTemporaryFile(suffix='.system')as f:
        tree.write(f.name)
        try:audit(f.name,model,report)
        except ValueError:rejected+=1
        else:raise ValueError('modem source injection mutation accepted')
    for name in ['mpss_smem_payload','mpss_smem_tail','ipa_smem_after']:
        targets=[i for i,o in enumerate(model['objects'])if re.fullmatch('frame_mr_'+name+r'_\d+',o['name'])]
        selected=targets if name=='mpss_smem_payload' else [targets[0],targets[-1]]
        selected=list(dict.fromkeys(selected))
        for index in selected:
            rights={'read':True,'write':name!='mpss_smem_payload'}
            for field,value in [('rights',rights),('cached',True),('executable',True)]:
                changed=copy.deepcopy(model)
                for obj in changed['objects']:
                    if not isinstance(obj['object'],dict):continue
                    for container in obj['object'].values():
                        if not isinstance(container,dict):continue
                        for slot in container.get('slots',[]):
                            cap=slot['cap'].get('Frame')
                            if cap and cap['object']==index:cap[field]=value
                try:audit(system,changed,report)
                except ValueError:rejected+=1
                else:raise ValueError('modem frame mutation accepted: '+name+'/'+str(index)+'/'+field)
    return rejected

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--system',type=Path,default=HERE/'native-startup-mpss-smem.system')
    p.add_argument('--capdl',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    p.add_argument('--dt',type=Path,action='append',default=[]);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();m=json.loads(a.capdl.read_text());r=a.report.read_text();result=audit(a.system,m,r,a.dt)
    result['mutations_rejected']=mutations(a.system,m,r)
    a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
