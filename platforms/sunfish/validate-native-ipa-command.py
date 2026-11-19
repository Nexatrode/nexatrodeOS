#!/usr/bin/env python3
"""Same bounded GSI/IPA frame authority for endpoint setup and channel-1 START."""
import argparse,importlib.util,json
from pathlib import Path
HERE=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('command_gsi_capdl',HERE/'validate-native-ipa-gsi.py')
gsi=importlib.util.module_from_spec(s);s.loader.exec_module(gsi)
def audit(system,model,report,dtbs=()):
    result=gsi.audit(system,model,report,dtbs)
    result.update(scope='One native PAS15/GSI setup, four AP endpoint configurations and empty command-channel-1 START; no TRE, transfer doorbell or MPSS/WLAN boot',
        host_channel_start=True,command_channel=1,command_event=0,endpoint_ids=[1,6,8,9],
        physical_start_completion_proven=False,functional_dma_proven=False)
    return result
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--system',type=Path,default=HERE/'native-startup-ipa-command.system')
    p.add_argument('--capdl',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    p.add_argument('--dt',type=Path,action='append',default=[]);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();model=json.loads(a.capdl.read_text());report=a.report.read_text()
    result=audit(a.system,model,report,a.dt);result['mutations_rejected']=gsi.mutations(a.system,model,report)
    a.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
