#!/usr/bin/env python3
"""Audit the unchanged bounded frame/IRQ authority for the two fixed memory/table DMA transactions."""
import argparse,importlib.util,json
from pathlib import Path
HERE=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('tables_transfer_capdl',HERE/'validate-native-ipa-transfer.py')
base=importlib.util.module_from_spec(s);s.loader.exec_module(base)
def audit(system,model,report,dtbs=()):
    result=base.audit(system,model,report,dtbs)
    result.update(scope='One native startup, then four memory and six route/filter commands in two retained DMA transactions; no MPSS/WLAN start or flash',
        host_transfer=True,host_channel_start=True,transfer_transactions=2,transfer_tres=10,table_commands=6,
        functional_dma_proven=False,production_ready=False)
    return result
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--system',type=Path,default=HERE/'native-startup-ipa-tables.system')
    p.add_argument('--capdl',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    p.add_argument('--dt',type=Path,action='append',default=[]);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();model=json.loads(a.capdl.read_text());report=a.report.read_text()
    result=audit(a.system,model,report,a.dt);result['mutations_rejected']=base.base.gsi.mutations(a.system,model,report)
    a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
