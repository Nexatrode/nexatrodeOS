#!/usr/bin/env python3
"""Audit exact native IPA PAS15 capability authority and inherited mappings."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path
import re
import tempfile
import xml.etree.ElementTree as ET

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location("ipa_firmware_shared",HERE/"validate-native-ipa-shared.py")
shared=importlib.util.module_from_spec(spec);spec.loader.exec_module(shared)
base=shared.base
base.PROFILE.update({"ipa_pas_metadata":(None,0x75601000,8192,"rw",False),
                     "ipa_fw_target":(0x98f80000,0x75800000,65536,"rw",False)})
base.EXTRA_PHYSICAL_SYMBOLS+=("ipa_pas_metadata","ipa_fw_target")
base.CONTIGUOUS_PRIVATE_REGIONS+=("ipa_pas_metadata",)
base.EXTRA_GUARD_PAGES=(0x75600000,0x75603000,0x757ff000,0x75810000)


def audit(system,model,report,dtbs=()):
    result=base.audit(system,model,report,dtbs,True,True,True,True,smc_allowed=True)
    result.update({"scope":"One native IPA PAS15 start with retained real shared maps; no host GSI/DMA command or modem/WLAN start",
                   "smc_slot":7,"smc_capabilities":1,"metadata_frames":2,"metadata_bytes":8192,
                   "metadata_contiguous_private":True,"firmware_frames":16,"firmware_physical":"0x98f80000",
                   "firmware_bytes":65536,"new_unmapped_guard_pages":4,"table_frames":7,
                   "smem_payload_access":False,"imem_payload_access":False,"private_pool_pages":32,
                   "host_gsi_writes":False,"functional_dma_proven":False,"hardware_pas_proven":False})
    return result


def mutations(system,model,report,audit_fn=None):
    verify=audit if audit_fn is None else audit_fn
    rejected=shared.mutations(system,model,report,audit_fn=verify)
    cases=[]
    for name in ["ipa_pas_metadata","ipa_fw_target"]:
        cases += [(name,"cached",True),(name,"executable",True),(name,"rights",{"read":True,"write":False})]
    for name,field,value in cases:
        changed=copy.deepcopy(model)
        target=next(i for i,o in enumerate(changed["objects"])if re.fullmatch("frame_mr_"+name+r"_\d+",o["name"]))
        for obj in changed["objects"]:
            if not isinstance(obj["object"],dict):continue
            for container in obj["object"].values():
                if not isinstance(container,dict):continue
                for entry in container.get("slots",[]):
                    cap=entry["cap"].get("Frame")
                    if cap and cap["object"]==target:cap[field]=value
        try:verify(system,changed,report)
        except ValueError:rejected+=1
        else:raise ValueError("firmware frame mutation accepted: "+name+"/"+field)
    for kind in ["remove","duplicate","wrong_slot"]:
        changed=copy.deepcopy(model)
        cnode=next(o["object"]["CNode"] for o in changed["objects"]if o["name"]=="cnode_native_startup_preflight")
        cap=next(e for e in cnode["slots"]if "ArmSmc" in e["cap"])
        if kind=="remove":cnode["slots"].remove(cap)
        elif kind=="duplicate":extra=copy.deepcopy(cap);extra["slot"]=8;cnode["slots"].append(extra)
        else:cap["slot"]=8
        try:verify(system,changed,report)
        except ValueError:rejected+=1
        else:raise ValueError("SMC capability mutation accepted: "+kind)
    for kind in ["target_physical","metadata_size","smc_disabled","metadata_symbol"]:
        tree=ET.parse(system)
        if kind=="target_physical":tree.find("memory_region[@name='ipa_fw_target']").set("phys_addr","0x98f90000")
        elif kind=="metadata_size":tree.find("memory_region[@name='ipa_pas_metadata']").set("size","0x3000")
        elif kind=="smc_disabled":tree.find("protection_domain").set("smc","false")
        else:tree.find("protection_domain/setvar[@symbol='ipa_pas_metadata_paddr']").set("region_paddr","smmu_data")
        with tempfile.NamedTemporaryFile(suffix=".system") as f:
            tree.write(f.name)
            try:verify(f.name,model,report)
            except ValueError:rejected+=1
            else:raise ValueError("firmware XML mutation accepted: "+kind)
    return rejected


if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__);p.add_argument("--system",type=Path,default=HERE/"native-startup-ipa-firmware.system")
    p.add_argument("--capdl",type=Path,required=True);p.add_argument("--report",type=Path,required=True)
    p.add_argument("--dt",action="append",type=Path,default=[]);p.add_argument("--output",type=Path,required=True)
    args=p.parse_args();model=json.loads(args.capdl.read_text());report=args.report.read_text()
    result=audit(args.system,model,report,args.dt);result["mutations_rejected"]=mutations(args.system,model,report)
    args.output.write_text(json.dumps(result,indent=2)+"\n");print(json.dumps(result,indent=2))
