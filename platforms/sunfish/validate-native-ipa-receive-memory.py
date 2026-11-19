#!/usr/bin/env python3
"""Actual CapDL audit for eight SMMU tables and the full receive CPU span."""
import argparse,copy,importlib.util,json,re,tempfile
from pathlib import Path
import xml.etree.ElementTree as E
HERE=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('receive_memory_tables',HERE/'validate-native-ipa-tables.py')
tables=importlib.util.module_from_spec(s);s.loader.exec_module(tables)
gsi=tables.base.base.gsi
base=gsi.base
base.DMA_PROFILE['smmu_l3_receive']=(None,0x7100f000,4096,'rw',False)
base.PROFILE['ipa_receive_data']=(None,0x76001000,0x200000,'rw',False)
base.EXTRA_PHYSICAL_SYMBOLS+=('ipa_receive_data',)
base.CONTIGUOUS_PRIVATE_REGIONS+=('ipa_receive_data',)
base.EXTRA_GUARD_PAGES+=(0x76000000,0x76201000)

def audit(system,model,report,dtbs=()):
    result=tables.audit(system,model,report,dtbs)
    result.update(scope='Native startup, two proven command transactions, eight-table receive mapping and CPU preparation only; no RX channel START, receive TRE/doorbell, packet access or modem/WLAN startup',
        table_frames=8,leaf_tables=4,expected_iommu_mapped_pages=539,
        receive_cpu_pages=512,receive_dma_pages=502,receive_cpu_only_tail_pages=10,
        receive_iova='0x80200000',receive_cpu_virtual='0x76001000',
        receive_cpu_contiguous=True,receive_cpu_cached=False,receive_cpu_executable=False,
        receive_start=False,receive_doorbell=False,physical_receive_mapping_proven=False,
        physical_receive_dma_proven=False,production_ready=False)
    return result

def mutations(system,model,report):
    rejected=gsi.mutations(system,model,report)
    for name,field,value in [
        ('ipa_receive_data','perms','r'),('ipa_receive_data','cached','true'),
        ('ipa_receive_data','vaddr','0x76002000'),('smmu_l3_receive','perms','r'),
        ('smmu_l3_receive','cached','true'),('smmu_l3_receive','setvar_vaddr','wrong_symbol')]:
        tree=E.parse(system);tree.find(f"protection_domain/map[@mr='{name}']").set(field,value)
        with tempfile.NamedTemporaryFile(suffix='.system')as f:
            tree.write(f.name)
            try:audit(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError('receive XML mutation accepted: '+name+'/'+field)
    for name in ['ipa_receive_data','smmu_l3_receive']:
        tree=E.parse(system);tree.find(f"protection_domain/setvar[@symbol='{name}_paddr']").set('region_paddr','smmu_data')
        with tempfile.NamedTemporaryFile(suffix='.system')as f:
            tree.write(f.name)
            try:audit(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError('receive physical symbol mutation accepted: '+name)
    for name in ['ipa_receive_data','smmu_l3_receive']:
        targets=[i for i,o in enumerate(model['objects'])if re.fullmatch('frame_mr_'+name+r'_\d+',o['name'])]
        indexes=[0]if len(targets)==1 else [0,501,502,511]
        for nth in indexes:
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
                else:raise ValueError('receive frame mutation accepted: '+name+'/'+str(nth)+'/'+field)
    return rejected

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--system',type=Path,default=HERE/'native-startup-ipa-receive.system')
    p.add_argument('--capdl',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    p.add_argument('--dt',type=Path,action='append',default=[]);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();m=json.loads(a.capdl.read_text());r=a.report.read_text();result=audit(a.system,m,r,a.dt)
    result['mutations_rejected']=mutations(a.system,m,r)
    a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
