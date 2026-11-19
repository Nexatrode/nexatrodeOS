#!/usr/bin/env python3
"""Actual CapDL audit for combined retained pre-auth power/QMP/SMP2P."""
from pathlib import Path
import argparse,copy,importlib.util,json,re,tempfile
import xml.etree.ElementTree as E
HERE=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('preauth_smem_capdl',HERE/'validate-native-mpss-smem.py')
smem=importlib.util.module_from_spec(s);s.loader.exec_module(smem);base=smem.base
base.PROFILE['mpss_aop_msgram']=(0x0c300000,0x77400000,0x1000,'rw',False)
base.PROFILE['mpss_apcs']=(0x17c00000,0x77402000,0x1000,'rw',False)
base.EXTRA_PHYSICAL_SYMBOLS+=('mpss_aop_msgram','mpss_apcs')
base.EXTRA_GUARD_PAGES+=(0x773ff000,0x77401000,0x77403000)
def audit(system,model,report,dtbs=()):
    result=smem.audit(system,model,report,dtbs)
    result.update(scope='Retained IPA/RMTFS/SMEM plus selected modem power/QMP and AP SMP2P publication. No PAS4, GLINK publication, modem/WLAN start, new DMA transfer or flash.',
        aop_msgram_physical='0x0c300000',apcs_physical='0x17c00000',apcs_software_write_bits=[0,14],
        added_irq_authority=False,added_smc_authority=False,apcs_page_authority='root only; fixed software whitelists must enforce subpage/bit limits',
        qmp_payload_bytes=64,smp2p_activated=True,glink_started=False,mpss_start=False,
        physical_modem_power_proven=False,physical_modem_smem_proven=False,production_ready=False)
    return result
def mutations(system,model,report):
    rejected=smem.mutations(system,model,report)
    for name in ['mpss_aop_msgram','mpss_apcs']:
        for field,value in [('perms','r'),('cached','true'),('vaddr','0x77301000'),('setvar_vaddr','wrong')]:
            tree=E.parse(system);tree.find(f"protection_domain/map[@mr='{name}']").set(field,value)
            with tempfile.NamedTemporaryFile(suffix='.system')as f:
                tree.write(f.name)
                try:audit(f.name,model,report)
                except ValueError:rejected+=1
                else:raise ValueError('preauth XML mutation accepted: '+name+'/'+field)
        tree=E.parse(system);tree.find(f"protection_domain/setvar[@symbol='{name}_paddr']").set('region_paddr','rmtfs_scm')
        with tempfile.NamedTemporaryFile(suffix='.system')as f:
            tree.write(f.name)
            try:audit(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError('preauth injection mutation accepted: '+name)
        tree=E.parse(system);tree.find(f"memory_region[@name='{name}']").set('phys_addr','0x17c01000')
        with tempfile.NamedTemporaryFile(suffix='.system')as f:
            tree.write(f.name)
            try:audit(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError('preauth physical mutation accepted: '+name)
        target=next(i for i,o in enumerate(model['objects'])if re.fullmatch('frame_mr_'+name+r'_\d+',o['name']))
        for field,value in [('rights',{'read':True,'write':False}),('cached',True),('executable',True)]:
            changed=copy.deepcopy(model)
            for obj in changed['objects']:
                if not isinstance(obj['object'],dict):continue
                for container in obj['object'].values():
                    if not isinstance(container,dict):continue
                    for slot in container.get('slots',[]):
                        cap=slot['cap'].get('Frame')
                        if cap and cap['object']==target:cap[field]=value
            try:audit(system,changed,report)
            except ValueError:rejected+=1
            else:raise ValueError('preauth frame mutation accepted: '+name+'/'+field)
    return rejected
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--system',type=Path,default=HERE/'native-startup-mpss-preauth.system');p.add_argument('--capdl',type=Path,required=True);p.add_argument('--report',type=Path,required=True);p.add_argument('--dt',type=Path,action='append',default=[]);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();m=json.loads(a.capdl.read_text());r=a.report.read_text();result=audit(a.system,m,r,a.dt);result['mutations_rejected']=mutations(a.system,m,r)
    a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
