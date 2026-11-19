#!/usr/bin/env python3
"""Actual frame/IRQ audit for bounded native exception receive diagnostics."""
from pathlib import Path
import argparse,importlib.util,json
HERE=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('native_receive_memory_capdl',HERE/'validate-native-ipa-receive-memory.py')
base=importlib.util.module_from_spec(s);s.loader.exec_module(base)
def audit(system,model,report,dtbs=()):
    result=base.audit(system,model,report,dtbs)
    result.update(scope='Native IPA startup, memory/table DMA and bounded exception RX START/replenish/default route, with retained UC/suspend/GSI dispatch and diagnostic aggregate reads only; no MPSS/WLAN start, network delivery or flash',
        receive_start=True,receive_doorbell=True,receive_initial_buffers=251,receive_initial_doorbells=16,
        shared_ipa_gsi_dispatch=True,diagnostic_aggregate_reads=True,network_delivery=False,
        physical_receive_start_proven=False,physical_receive_dma_proven=False,production_ready=False)
    return result
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--system',type=Path,default=HERE/'native-startup-ipa-receive.system')
    p.add_argument('--capdl',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    p.add_argument('--dt',type=Path,action='append',default=[]);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();model=json.loads(a.capdl.read_text());report=a.report.read_text();result=audit(a.system,model,report,a.dt)
    result['mutations_rejected']=base.mutations(a.system,model,report)
    a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
