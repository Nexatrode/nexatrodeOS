#!/usr/bin/env python3
"""Actual capability audit for the seven-table, private-RAM root experiment."""
from pathlib import Path
import argparse
import importlib.util
import json

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location("base_smmu_audit",HERE/"validate-smmu-install-probe.py")
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
base.PROFILE.update({"smmu_l2_peer":(None,0x71009000),"smmu_l3_peer":(None,0x7100b000),
                     "smmu_l3_shared":(None,0x7100d000)})
AuditError=base.AuditError
audit=base.audit

if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--system',type=Path,default=HERE/'smmu-ipa-root-install-probe.system')
    p.add_argument('--capdl',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    p.add_argument('--dt',type=Path,action='append',default=[]);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--data-pages',type=int,choices=[37],default=37)
    a=p.parse_args();r=audit(a.system,json.loads(a.capdl.read_text()),a.report.read_text(),a.dt,37)
    r.update({'table_frames':7,'root_regions':2,'leaf_regions':3,'private_ram_only':True,
              'actual_imem_mapped':False,'actual_smem_mapped':False})
    a.output.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r,indent=2))
