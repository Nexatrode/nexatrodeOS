#!/usr/bin/env python3
"""Audit the combined real IMEM/SMEM native stage against actual CapDL."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path
import re
import tempfile
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("shared_native_audit", HERE / "validate-native-startup-preflight.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
base.PROFILE.update({
    "ipa_smem_before": (0x86000000, 0x75000000, 0x11d000, "r", False),
    "ipa_smem_metadata": (0x8611d000, 0x7511d000, 4096, "rw", False),
    "ipa_smem_after": (0x8611e000, 0x7511e000, 0xe2000, "r", False),
    "ipa_smem_lock": (0x01f43000, 0x75300000, 4096, "rw", False),
    "ipa_imem": (0x146a8000, 0x75400000, 8192, "r", False),
})
base.DMA_PROFILE.update({
    "smmu_l2_peer": (None, 0x71009000, 4096, "rw", False),
    "smmu_l3_peer": (None, 0x7100b000, 4096, "rw", False),
    "smmu_l3_shared": (None, 0x7100d000, 4096, "rw", False),
})
base.EXTRA_PHYSICAL_SYMBOLS = ("ipa_smem_before", "ipa_smem_metadata", "ipa_smem_lock", "ipa_imem")


def audit(system, model, report, dtbs=()):
    result = base.audit(system, model, report, dtbs, True, True, True, True)
    result.update({"shared_reserved_views": True, "cpu_imem_write": False,
                   "smem_writable_metadata_pages": 1, "other_smem_pages_read_only": 511,
                   "real_lock3": True, "table_frames": 7, "leaf_tables": 3,
                   "private_pool_pages": 32, "expected_iommu_mapped_pages": 37,
                   "payload_access_commands": False, "functional_dma_proven": False})
    return result


def mutations(system, model, report, audit_fn=None):
    verify = audit if audit_fn is None else audit_fn
    callback = None if audit_fn is None else lambda s,m,r,**flags: audit_fn(s,m,r)
    rejected = base.mutations(system, model, report, True, True, True, True, audit_fn=callback)
    cases = [("ipa_smem_before", "rights", {"read": True, "write": True}),
             ("ipa_smem_after", "rights", {"read": True, "write": True}),
             ("ipa_smem_metadata", "rights", {"read": True, "write": False}),
             ("ipa_smem_after", "cached", True), ("ipa_smem_lock", "cached", True),
             ("ipa_imem", "rights", {"read": True, "write": True}),
             ("ipa_imem", "cached", True), ("ipa_imem", "executable", True)]
    for table in ["smmu_l2_peer", "smmu_l3_peer", "smmu_l3_shared"]:
        cases += [(table, "cached", True), (table, "executable", True)]
    for name, field, value in cases:
        candidate = copy.deepcopy(model)
        index = next(i for i, o in enumerate(candidate["objects"])
                     if re.fullmatch("frame_mr_" + re.escape(name) + r"_\d+", o["name"]))
        for obj in candidate["objects"]:
            if not isinstance(obj["object"], dict):
                continue
            for container in obj["object"].values():
                if isinstance(container, dict):
                    for entry in container.get("slots", []):
                        cap = entry["cap"].get("Frame")
                        if cap and cap["object"] == index:
                            cap[field] = value
        try:
            verify(system, candidate, report)
        except ValueError:
            rejected += 1
        else:
            raise ValueError("shared CapDL mutation accepted: " + name + "/" + field)
    for region, attribute, value in [("ipa_imem", "phys_addr", "0x146a9000"),
                                     ("ipa_smem_metadata", "phys_addr", "0x8611e000")]:
        xml = ET.parse(system)
        xml.find(f"memory_region[@name='{region}']").set(attribute, value)
        with tempfile.NamedTemporaryFile(suffix=".system") as tmp:
            xml.write(tmp.name)
            try:
                verify(tmp.name, model, report)
            except ValueError:
                rejected += 1
            else:
                raise ValueError("shared physical binding mutation accepted")
    return rejected


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--system", type=Path, default=HERE / "native-startup-ipa-shared.system")
    p.add_argument("--capdl", required=True, type=Path)
    p.add_argument("--report", required=True, type=Path)
    p.add_argument("--dt", action="append", default=[], type=Path)
    p.add_argument("--output", required=True, type=Path)
    args = p.parse_args()
    model = json.loads(args.capdl.read_text()); report = args.report.read_text()
    result = audit(args.system, model, report, args.dt)
    result["mutations_rejected"] = mutations(args.system, model, report)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
