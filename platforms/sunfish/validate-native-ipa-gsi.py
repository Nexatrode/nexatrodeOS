#!/usr/bin/env python3
"""Audit exact five-page GSI write authority and physical interrupt binding."""
import argparse,copy,importlib.util,json,re,tempfile
from pathlib import Path
import xml.etree.ElementTree as E
HERE=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('native_gsi_firmware',HERE/'validate-native-ipa-firmware.py');firmware=importlib.util.module_from_spec(s);s.loader.exec_module(firmware)
base=firmware.base
WINDOWS={'gsi_regs':(0,0xc000,'r'),'gsi_inter_ee':(0xc000,0x1000,'rw'),
    'gsi_reserved_middle':(0xd000,0xf000,'r'),'gsi_context_channel':(0x1c000,0x1000,'rw'),
    'gsi_context_event':(0x1d000,0x1000,'rw'),'gsi_doorbell':(0x1e000,0x1000,'rw'),
    'gsi_control':(0x1f000,0x1000,'rw'),'gsi_reserved_tail':(0x20000,0xc000,'r')}
base.PROFILE.update({n:(0x1e04000+off,0x44100000+off,size,rights,False) for n,(off,size,rights) in WINDOWS.items()})

def audit(system,model,report,dtbs=()):
    base.require(E.parse(system).find("protection_domain/map[@mr='gsi_regs']").get('setvar_vaddr')=='gsi_regs','GSI base symbol injection')
    v=base.audit(system,model,report,dtbs,True,True,True,True,smc_allowed=True,extra_irqs=((464,3,'level'),))
    v.update({'scope':'One PAS15 start and four AP/four modem-EE GSI allocation/context setup; no channel START, TRE transfer or MPSS/WLAN start',
        'smc_slot':7,'smc_capabilities':1,'gsi_irq':464,'gsi_irq_channel':3,'gsi_irq_trigger':'level',
        'gsi_writable_pages':[hex(off)for off,size,rights in WINDOWS.values()if rights=='rw'],
        'gsi_readonly_pages':39,'all_gsi_read_only':False,'other_ipa_and_all_gsi_read_only':False,
        'private_pool_pages':32,'table_frames':7,'metadata_frames':2,'firmware_frames':16,
        'physical_gsi_delivery_proven':False,'functional_dma_proven':False,'host_channel_start':False})
    return v

def mutations(system,model,report):
    rejected=firmware.mutations(system,model,report,audit_fn=audit)
    for name,(_,_,rights) in WINDOWS.items():
        for field,value in [('rights',{'read':True,'write':rights!='rw'}),('cached',True),('executable',True)]:
            changed=copy.deepcopy(model)
            index=next(i for i,o in enumerate(changed['objects'])if re.fullmatch('frame_mr_'+name+r'_\d+',o['name']))
            for obj in changed['objects']:
                if not isinstance(obj['object'],dict):continue
                for c in obj['object'].values():
                    if isinstance(c,dict):
                        for slot in c.get('slots',[]):
                            cap=slot['cap'].get('Frame')
                            if cap and cap['object']==index:cap[field]=value
            try:audit(system,changed,report)
            except ValueError:rejected+=1
            else:raise ValueError('GSI mutation accepted: '+name+'/'+field)
    for field in ['trigger','badge','destination','channel']:
        changed=copy.deepcopy(model);h=next(x['handler']for x in changed['irqs']if x['irq']==464);irq=changed['objects'][h]['object']['ArmIrq']
        if field=='trigger':irq['extra']['trigger']=1
        elif field=='badge':irq['slots'][0]['cap']['Notification']['badge']=4
        elif field=='destination':irq['slots'][0]['cap']['Notification']['object']=0
        else:
            c=next(o['object']['CNode']for o in changed['objects']if o['name']=='cnode_native_startup_preflight')
            slot=next(x for x in c['slots']if x['slot']==141);slot['slot']=142
        try:audit(system,changed,report)
        except ValueError:rejected+=1
        else:raise ValueError('GSI IRQ mutation accepted: '+field)
    for name in WINDOWS:
        tree=E.parse(system);tree.find("memory_region[@name='"+name+"']").set('phys_addr','0x1e30000')
        with tempfile.NamedTemporaryFile(suffix='.system')as f:
            tree.write(f.name)
            try:audit(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError('GSI physical mutation accepted')
    return rejected

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--system',type=Path,default=HERE/'native-startup-ipa-gsi.system')
    p.add_argument('--capdl',type=Path,required=True);p.add_argument('--report',type=Path,required=True);p.add_argument('--dt',type=Path,action='append',default=[]);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();m=json.loads(a.capdl.read_text());r=a.report.read_text();v=audit(a.system,m,r,a.dt);v['mutations_rejected']=mutations(a.system,m,r)
    a.output.write_text(json.dumps(v,indent=2)+'\n');print(json.dumps(v,indent=2))
