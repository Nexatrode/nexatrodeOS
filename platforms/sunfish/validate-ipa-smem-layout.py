#!/usr/bin/env python3
"""Audit the exact authority of the read-only SMEM partition-layout observer."""
from pathlib import Path
import argparse
import copy
import importlib.util
import json
import re
import tempfile
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
module_spec = importlib.util.spec_from_file_location("base_memory_authority", HERE / "validate-native-startup-preflight.py")
base = importlib.util.module_from_spec(module_spec)
module_spec.loader.exec_module(base)
# Reuse the general actual-CapDL frame, alias, CNode, page-table, NX/cache,
# reservation and authority checks with this exact three-region profile.
# No IPA startup profile or its SMMU/IRQ/SMC authority is enabled here.
base.PROFILE = {
    "smem_snapshot": (0x86000000, 0x40000000, 0x200000, "r", False),
    "tcsr_smem_lock": (0x01f43000, 0x41000000, 0x1000, "rw", False),
    "splash": (0x9c000000, 0x42000000, 0x1600000, "rw", False),
}
SYMBOLS = {"smem_snapshot": "smem_vaddr", "tcsr_smem_lock": "tcsr_lock_vaddr", "splash": "splash_vaddr"}


def audit(system, spec, report, dtbs=()):
    pd = ET.parse(system).getroot().find("protection_domain")
    base.require(pd is not None and pd.find("program_image").get("path") == "ipa-smem-layout.elf", "census image binding")
    base.require(not pd.findall("setvar"), "no extra physical symbol authority")
    for name, symbol in SYMBOLS.items():
        m = pd.find(f"map[@mr='{name}']")
        base.require(m is not None and m.get("setvar_vaddr") == symbol, "exact virtual injection: " + name)
    result = base.audit(system, spec, report, dtbs)
    return {"scope": "Full table and item-497 metadata reads only; TCSR hardware lock 3 acquisition/release and framebuffer writes; no allocation, payload reads, IPA, SMMU, DMA, PAS, SMC or flash",
            "selected_frames": result["selected_frames"], "private_guard_pages": result["guard_pages_unmapped"],
            "smem_bytes": 0x200000, "smem_writable": False, "smem_cached": False,
            "lock_physical": "0x01f43000", "lock_page_bytes": 4096,
            "smc": False, "irqs": [], "ipc": False, "reset_pd": False}


def mutations(system, spec, report):
    rejected = 0
    for name, attribute, value in [("smem_snapshot", "perms", "rw"), ("smem_snapshot", "cached", "true"),
                                    ("tcsr_smem_lock", "perms", "r"), ("tcsr_smem_lock", "setvar_vaddr", "smem_vaddr")]:
        tree = ET.parse(system);tree.find(f"protection_domain/map[@mr='{name}']").set(attribute, value)
        with tempfile.NamedTemporaryFile(suffix=".system") as f:
            tree.write(f.name)
            try: audit(f.name, spec, report)
            except ValueError: rejected += 1
            else: raise ValueError("XML mutation accepted: " + name)
    tree=ET.parse(system);tree.find("protection_domain").set("smc","true")
    with tempfile.NamedTemporaryFile(suffix=".system") as f:
        tree.write(f.name)
        try: audit(f.name,spec,report)
        except ValueError: rejected += 1
        else: raise ValueError("SMC XML mutation accepted")
    for name,field,value in [("smem_snapshot","rights",{"read":True,"write":True}),
                              ("smem_snapshot","executable",True),("smem_snapshot","cached",True),
                              ("tcsr_smem_lock","cached",True)]:
        changed=copy.deepcopy(spec)
        target=next(i for i,o in enumerate(changed["objects"]) if re.fullmatch("frame_mr_"+re.escape(name)+r"_\d+",o["name"]))
        for o in changed["objects"]:
            if not isinstance(o["object"],dict):continue
            for c in o["object"].values():
                if not isinstance(c,dict):continue
                for entry in c.get("slots",[]):
                    cap=entry["cap"].get("Frame")
                    if cap and cap["object"]==target:cap[field]=value
        try:audit(system,changed,report)
        except ValueError:rejected+=1
        else:raise ValueError("CapDL mutation accepted: "+name+" "+field)
    changed=copy.deepcopy(spec);changed["irqs"]=[{"irq":343}]
    try:audit(system,changed,report)
    except ValueError:rejected+=1
    else:raise ValueError("IRQ authority accepted")
    changed=copy.deepcopy(spec)
    cnode=next(o["object"]["CNode"] for o in changed["objects"] if o["name"]=="cnode_native_startup_preflight")
    cnode["slots"].append({"slot":511,"cap":{"ArmSmc":{}}})
    try:audit(system,changed,report)
    except ValueError:rejected+=1
    else:raise ValueError("SMC capability accepted")
    return rejected


if __name__ == "__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--system",type=Path,default=HERE/"ipa-smem-layout.system")
    p.add_argument("--capdl",type=Path,required=True);p.add_argument("--report",type=Path,required=True)
    p.add_argument("--dt",type=Path,action="append",default=[]);p.add_argument("--output",type=Path,required=True)
    args=p.parse_args();spec=json.loads(args.capdl.read_text());report=args.report.read_text()
    result=audit(args.system,spec,report,args.dt);result["mutations_rejected"]=mutations(args.system,spec,report)
    args.output.write_text(json.dumps(result,indent=2)+"\n");print(json.dumps(result,indent=2))
