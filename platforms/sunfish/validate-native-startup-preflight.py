#!/usr/bin/env python3
"""Audit real CapDL authority of the private native IPA prestart image."""
import argparse
import copy
import importlib.util
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET

# physical, virtual, bytes, rights, cached
PROFILE = {
    "apps_rsc": (0x18220000, 0x40000000, 0x1000, "rw", False),
    "apps_rsc_tail": (0x18221000, 0x40001000, 0x2000, "r", False),
    "cmd_db_dictionary": (0x0c3f0000, 0x41000000, 0x1000, "r", False),
    "cmd_db_snapshot": (0x85f20000, 0x41001000, 0x2000, "r", False),
    "ipa_regs": (0x01e40000, 0x44000000, 0x7000, "r", False),
    "gsi_regs": (0x01e04000, 0x44100000, 0x2c000, "r", False),
    "efs_shadow1": (None, 0x74001000, 0x200000, "rw", True),
    "efs_shadow2": (None, 0x74203000, 0x200000, "rw", True),
    "efs_shadow3": (None, 0x74405000, 0x200000, "rw", True),
    "efs_shadow4": (None, 0x74607000, 0x20000, "rw", True),
    "splash": (0x9c000000, 0x42000000, 0x1600000, "rw", False),
}

# Same retained-bank aperture and guarded private pool as the standalone
# experiment; the union is checked directly against the actual CapDL model.
DMA_PROFILE = {
    "smmu_gr0": (0x15000000, 0x70000000, 0x1000, "rw", False),
    "smmu_gr1": (0x15001000, 0x70001000, 0x1000, "rw", False),
    "smmu_context": (0x15084000, 0x70002000, 0x1000, "rw", False),
    "smmu_cb_low": (0x15080000, 0x73000000, 0x4000, "r", False),
    "smmu_cb_high": (0x15085000, 0x73010000, 0x38000, "r", False),
    "smmu_root": (None, 0x71001000, 0x1000, "rw", False),
    "smmu_l2": (None, 0x71003000, 0x1000, "rw", False),
    "smmu_l3": (None, 0x71005000, 0x1000, "rw", False),
    "smmu_upper": (None, 0x71007000, 0x1000, "rw", False),
    "smmu_data": (None, 0x72001000, 0x20000, "rw", False),
}
EXTRA_PHYSICAL_SYMBOLS = ()
EXTRA_GUARD_PAGES = ()
CONTIGUOUS_PRIVATE_REGIONS = ("smmu_data",)
FRAME_BITS = {}
EXTRA_IRQS = ()
UNMAPPED_RESERVED_PROFILE = {}
AUDIT_ALL_FRAMES_FOR_RESERVATIONS = False
EXTRA_SELF_CHANNEL = None

def require(value, message):
    if not value: raise ValueError(message)

def selected_profile(hardware_config, dma_domain=False, local_memory=False, uc_irq=False):
    profile = dict(PROFILE)
    if hardware_config:
        profile["ipa_regs"] = (0x01e40000, 0x44000000, 0x1000, "rw", False)
        profile["ipa_regs_tail"] = (0x01e41000, 0x44001000, 0x6000, "r", False)
    if dma_domain:
        require(hardware_config, "domain requires primitive configuration profile")
        profile.update(DMA_PROFILE)
    if local_memory:
        require(dma_domain, "local memory requires the retained domain profile")
        profile["ipa_shared"] = (0x01e47000, 0x44200000, 0x2000, "rw", False)
    if uc_irq:
        require(local_memory, "UC IRQ profile requires retained SRAM and domain")
        profile["ipa_regs_tail"] = (0x01e41000, 0x44001000, 0x2000, "r", False)
        profile["ipa_irq_regs"] = (0x01e43000, 0x44003000, 0x1000, "rw", False)
        profile["ipa_regs_high"] = (0x01e44000, 0x44004000, 0x3000, "r", False)
    profile.update(UNMAPPED_RESERVED_PROFILE)
    return profile

def audit(system, spec, report, dtbs=(), hardware_config=False, dma_domain=False, local_memory=False, uc_irq=False, smc_allowed=False, extra_irqs=()):
    profile = selected_profile(hardware_config, dma_domain, local_memory, uc_irq)
    tree = ET.parse(system).getroot()
    regions = {r.get("name"): r for r in tree.findall("memory_region")}
    require(set(regions) == set(profile) and len(tree.findall("memory_region")) == len(profile), "exact region set")
    pds = tree.findall("protection_domain")
    require(len(pds) == 1 and pds[0].get("name") == "native_startup_preflight", "single native owner")
    pd = pds[0]
    channels = tree.findall("channel")
    if EXTRA_SELF_CHANNEL is None:
        require(not channels, "no IPC")
    else:
        require(len(channels) == 1 and len(channels[0]) == 2, "one exact self-channel")
        ends = [end.attrib for end in channels[0]]
        require(all(end.tag == "end" for end in channels[0]) and ends == list(EXTRA_SELF_CHANNEL),
                "exact self-channel endpoints")
    require(pd.get("smc") == str(smc_allowed).lower(), "exact SMC policy")
    if dma_domain:
        require(int(pd.get("stack_size", "0"), 0) >= 0x100000, "table materialization stack")
        expected_irqs = sorted([(97, 1, "level"), (132, 0, "level")] + ([(343, 2, "edge")] if uc_irq else []) + list(extra_irqs) + list(EXTRA_IRQS))
        require(sorted((int(q.get("irq")), int(q.get("id")), q.get("trigger")) for q in pd.findall("irq"))
                == expected_irqs, "exact XML IRQs")
        symbol_names = tuple(DMA_PROFILE) + EXTRA_PHYSICAL_SYMBOLS
        require(len(pd.findall("setvar")) == len(symbol_names), "exact physical symbol set")
        for name in symbol_names:
            symbols = pd.findall(f"setvar[@symbol='{name}_paddr']")
            require(len(symbols) == 1 and symbols[0].get("region_paddr") == name, "physical injection: " + name)
    else:
        require(not pd.findall("irq") and not spec["irqs"], "no IRQ authority")
    require(len(pd.findall("map")) == len(profile) - len(UNMAPPED_RESERVED_PROFILE), "exact map set")
    for name, (physical, virtual, size, rights, cached) in profile.items():
        region = regions[name]
        require(int(region.get("size"), 0) == size, "region size: " + name)
        bits = FRAME_BITS.get(name, 21 if name == "splash" else 12)
        require(int(region.get("page_size", "0x1000"), 0) == 1 << bits, "region page granule: " + name)
        require((int(region.get("phys_addr"), 0) if region.get("phys_addr") else None) == physical, "physical binding: " + name)
        maps = pd.findall(f"map[@mr='{name}']")
        if name in UNMAPPED_RESERVED_PROFILE:
            require(not maps and virtual is None and rights is None, "reservation has no owner mapping: " + name)
            continue
        require(len(maps) == 1, "one map: " + name)
        m = maps[0]
        require(int(m.get("vaddr"), 0) == virtual and m.get("perms") == rights and m.get("cached") == str(cached).lower(), "mapping geometry/rights: " + name)
        if name in DMA_PROFILE or name in EXTRA_PHYSICAL_SYMBOLS:
            require(m.get("setvar_vaddr") == name, "virtual injection: " + name)
        if name == "ipa_shared":
            require(m.get("setvar_vaddr") == name, "local memory virtual injection")
    objects = spec["objects"]
    addresses = {}
    for i, name, address in re.findall(r"^\s*(\d+) - [^:]+: '([^']+)' @ (0x[0-9a-fA-F]+)$", report, re.M):
        i = int(i); require(objects[i]["name"] == name and i not in addresses, "stale allocation report")
        addresses[i] = int(address, 16)
    selected = {}
    for name, (physical, virtual, size, rights, cached) in profile.items():
        bits = FRAME_BITS.get(name, 21 if name == "splash" else 12)
        frames = [(i, o) for i, o in enumerate(objects) if re.fullmatch("frame_mr_" + re.escape(name) + r"_\d+", o["name"])]
        require(len(frames) == size >> bits, "actual frame count: " + name)
        frames.sort(key=lambda x: addresses.get(x[0], -1))
        for page, (i, obj) in enumerate(frames):
            require(i in addresses and obj["object"]["Frame"]["size_bits"] == bits, "frame identity/granule")
            address = addresses[i]
            require(obj["object"]["Frame"]["paddr"] in [None, address], "frame/report identity")
            require(physical is None or address == physical + (page << bits), "actual fixed frame address")
            require(physical is not None or 0x80000000 <= address < 1 << 40, "owned RAM bounds")
            if name in CONTIGUOUS_PRIVATE_REGIONS:
                require(address == addresses[frames[0][0]] + (page << bits), "contiguous private pool physical pages")
            selected[i] = (name, address, None if virtual is None else virtual + (page << bits), 1 << bits, rights, cached)
    require(len({address for _, address, *_ in selected.values()}) == len(selected), "selected physical alias")
    references = {i: 0 for i in selected}
    all_frames = []
    smc_caps = []
    for i, obj in enumerate(objects):
        if not isinstance(obj["object"], dict): continue
        frame = obj["object"].get("Frame")
        if frame and i in addresses:
            start, size = addresses[i], 1 << frame["size_bits"]
            require(start >= 0x80000000 or i in selected, "extra device frame")
            all_frames.append((i, start, size))
        for container in obj["object"].values():
            if not isinstance(container, dict): continue
            for entry in container.get("slots", []):
                cap = entry["cap"]
                if any("smc" in kind.lower() for kind in cap):
                    require(smc_allowed and set(cap) == {"ArmSmc"}
                            and obj["name"] == "cnode_native_startup_preflight" and entry["slot"] == 7
                            and set(cap["ArmSmc"]) == {"object"}, "exact sole SMC capability")
                    target_object = cap["ArmSmc"]["object"]
                    require(objects[target_object]["object"] == "ArmSmc", "SMC target object")
                    smc_caps.append(target_object)
                target = cap.get("Frame", {}).get("object")
                if target in references: references[target] += 1
    # The sorted sweep checks every physical frame object, including ELF
    # frames. It also avoids a quadratic scan for the large modem image.
    ordered_frames = sorted(all_frames, key=lambda f: f[1])
    for previous, current in zip(ordered_frames, ordered_frames[1:]):
        require(previous[1] + previous[2] <= current[1], "overlapping frame object")
    require(all(n == (0 if selected[i][0] in UNMAPPED_RESERVED_PROFILE else 1) for i, n in references.items()), "frame capability alias")
    require(len(smc_caps) == int(smc_allowed), "exact SMC capability count")
    if smc_allowed:
        require(sum(o["object"] == "ArmSmc" for o in objects) == 1, "sole SMC object")
    owner = next(i for i, o in enumerate(objects) if o["name"] == "pgd_native_startup_preflight")
    found = {i: [] for i in selected}
    guards = [x for name, (_, va, size, _, _) in profile.items() if name.startswith("efs_shadow") for x in [va - 4096, va + size]]
    if dma_domain:
        guards += [x for name, (pa, va, size, _, _) in DMA_PROFILE.items() if pa is None for x in [va - 4096, va + size]]
    guards += list(EXTRA_GUARD_PAGES)
    def walk(root, index, level, at, ancestors):
        require(index not in ancestors and level <= 3, "page table cycle/depth")
        table = objects[index]["object"]["PageTable"]
        require(table["level"] == level, "page table level")
        for entry in table["slots"]:
            virtual = at + (entry["slot"] << (39 - level * 9)); cap = entry["cap"]
            if "PageTable" in cap: walk(root, cap["PageTable"]["object"], level + 1, virtual, ancestors | {index})
            elif "Frame" in cap:
                fc = cap["Frame"]; size = 1 << objects[fc["object"]]["object"]["Frame"]["size_bits"]
                if root == owner: require(not any(virtual <= g < virtual + size for g in guards), "mapped shadow guard")
                if fc["object"] in found: found[fc["object"]].append((root, virtual, fc))
    for i, o in enumerate(objects):
        if isinstance(o["object"], dict) and o["object"].get("PageTable", {}).get("is_root"):
            walk(i, i, 0, 0, set())
    for i, (name, _, va, _, rights, cached) in selected.items():
        if name in UNMAPPED_RESERVED_PROFILE:
            require(not found[i], "reservation mapped into an address space: " + name)
            continue
        require(len(found[i]) == 1, "mapped alias: " + name)
        root, actual, cap = found[i][0]
        require(root == owner and actual == va and cap["rights"]["read"] and cap["rights"]["write"] == (rights == "rw")
                and not cap["executable"] and cap["cached"] == cached, "actual mapping rights/cache: " + name)
    if dma_domain:
        cnodes = [o["object"]["CNode"] for o in objects if o["name"] == "cnode_native_startup_preflight"]
        require(len(cnodes) == 1, "owner CNode")
        slots = {entry["slot"]: entry["cap"] for entry in cnodes[0]["slots"]}
        require(sorted(q["irq"] for q in spec["irqs"]) == sorted([q[0] for q in expected_irqs]), "exact CapDL IRQs")
        for irq, channel, trigger in expected_irqs:
            handler = next(q["handler"] for q in spec["irqs"] if q["irq"] == irq)
            require(slots.get(138 + channel, {}).get("ArmIrqHandler", {}).get("object") == handler, "IRQ channel binding")
            description = objects[handler]["object"]["ArmIrq"]
            notice = description["slots"][0]["cap"]["Notification"]
            require(notice["object"] == slots[1]["Notification"]["object"] and notice["badge"] == 1 << channel
                    and description["extra"]["trigger"] == (1 if trigger == "edge" else 0), "IRQ destination/trigger")
    module_spec = importlib.util.spec_from_file_location("memory_audit", Path(__file__).with_name("audit-capdl-memory.py"))
    memory = importlib.util.module_from_spec(module_spec); module_spec.loader.exec_module(memory)
    for dtb in dtbs:
        exclusions, _ = memory.read_regions(dtb)
        for name, address, _, size, _, _ in selected.values():
            if profile[name][0] is None:
                for region, start, length, _ in exclusions:
                    require(not (address < start + length and start < address + size), name + " RAM overlaps " + region)
        if AUDIT_ALL_FRAMES_FOR_RESERVATIONS:
            for i, address, size in all_frames:
                if i in selected and profile[selected[i][0]][0] is not None:
                    continue  # Exact fixed frame geometry/rights checked above.
                for region, start, length, _ in exclusions:
                    require(not (address < start + length and start < address + size), objects[i]['name'] + " frame overlaps " + region)
    return {"scope": "private RAM providers, IPA power floors, optional primitive configuration, two-route SMMU domain and local-memory canaries; no PAS, DMA or modem/WLAN start",
            "selected_frames": len(selected), "shadow_bytes": 3 * 0x200000 + 0x20000,
            "guard_pages_unmapped": len(guards), "shadow_guards": 8, "ipa_first_page_writable": hardware_config,
            "ipa_irq_page_writable": uc_irq, "other_ipa_and_all_gsi_read_only": True,
            "smc": smc_allowed, "irqs": [q[0] for q in expected_irqs] if dma_domain else [], "ipc": False,
            "smmu_domain": dma_domain, "private_dma_pool_bytes": 0x20000 if dma_domain else 0,
            "local_memory_canaries": local_memory, "ipa_shared_bytes": 0x2000 if local_memory else 0,
            "smmu_data_physical": [hex(address) for name, address, *_ in selected.values() if name == "smmu_data"]}

def mutations(system, spec, report, hardware_config=False, dma_domain=False, local_memory=False, uc_irq=False, audit_fn=None):
    import tempfile
    verify = audit if audit_fn is None else audit_fn
    rejected = 0
    xml_cases = [("ipa_regs", "perms", "r" if hardware_config else "rw"),
                 ("gsi_regs", "perms", "rw"), ("efs_shadow1", "cached", "false")]
    if hardware_config: xml_cases.append(("ipa_regs_tail", "perms", "rw"))
    if dma_domain:
        xml_cases += [("smmu_cb_low", "perms", "rw"), ("smmu_data", "cached", "true"),
                      ("smmu_root", "setvar_vaddr", "wrong_symbol")]
    if local_memory:
        xml_cases += [("ipa_shared", "perms", "r"), ("ipa_shared", "cached", "true")]
    if uc_irq:
        xml_cases += [("ipa_irq_regs", "perms", "r"), ("ipa_regs_high", "perms", "rw")]
    for region, attribute, value in xml_cases:
        tree = ET.parse(system); tree.find(f"protection_domain/map[@mr='{region}']").set(attribute, value)
        with tempfile.NamedTemporaryFile(suffix=".system") as f:
            tree.write(f.name)
            try: verify(f.name, spec, report, hardware_config=hardware_config, dma_domain=dma_domain, local_memory=local_memory, uc_irq=uc_irq)
            except ValueError: rejected += 1
            else: raise ValueError("mutation accepted: " + region)
    cap_cases = [("ipa_regs", "rights", {"read": True, "write": not hardware_config}),
                 ("efs_shadow1", "executable", True), ("efs_shadow1", "cached", False)]
    if hardware_config: cap_cases.append(("ipa_regs_tail", "rights", {"read": True, "write": True}))
    if dma_domain:
        cap_cases += [("smmu_data", "cached", True), ("smmu_data", "executable", True),
                      ("smmu_root", "cached", True), ("smmu_cb_high", "rights", {"read": True, "write": True})]
    if local_memory:
        cap_cases += [("ipa_shared", "cached", True), ("ipa_shared", "executable", True)]
    if uc_irq:
        cap_cases += [("ipa_irq_regs", "rights", {"read": True, "write": False}),
                      ("ipa_irq_regs", "executable", True), ("ipa_irq_regs", "cached", True),
                      ("ipa_regs_high", "rights", {"read": True, "write": True})]
    for name, field, value in cap_cases:
        changed = copy.deepcopy(spec)
        target = next(i for i, o in enumerate(changed["objects"]) if re.fullmatch("frame_mr_" + re.escape(name) + r"_\d+", o["name"]))
        for obj in changed["objects"]:
            if not isinstance(obj["object"], dict): continue
            for container in obj["object"].values():
                if isinstance(container, dict):
                    for entry in container.get("slots", []):
                        cap = entry["cap"].get("Frame")
                        if cap and cap["object"] == target: cap[field] = value
        try: verify(system, changed, report, hardware_config=hardware_config, dma_domain=dma_domain, local_memory=local_memory, uc_irq=uc_irq)
        except ValueError: rejected += 1
        else: raise ValueError("CapDL mutation accepted: " + name)
    if dma_domain:
        changed = copy.deepcopy(spec); changed["irqs"][0]["irq"] = 98
        try: verify(system, changed, report, hardware_config=True, dma_domain=True, local_memory=local_memory, uc_irq=uc_irq)
        except ValueError: rejected += 1
        else: raise ValueError("wrong IRQ accepted")
        tree = ET.parse(system); tree.find("protection_domain/setvar[@symbol='smmu_data_paddr']").set("region_paddr", "smmu_root")
        with tempfile.NamedTemporaryFile(suffix=".system") as f:
            tree.write(f.name)
            try: verify(f.name, spec, report, hardware_config=True, dma_domain=True, local_memory=local_memory, uc_irq=uc_irq)
            except ValueError: rejected += 1
            else: raise ValueError("wrong physical injection accepted")
        pages = [(int(i), name, int(address,16)) for i, name, address in re.findall(
            r"^\s*(\d+) - [^:]+: '(frame_mr_smmu_data_\d+)' @ (0x[0-9a-fA-F]+)$", report, re.M)]
        index, name, address = max(pages, key=lambda entry: entry[2])
        changed_report = re.sub(r"('" + re.escape(name) + r"' @ )0x[0-9a-fA-F]+", lambda m: m[1]+hex(address+0x100000000), report, count=1)
        require(changed_report != report, "pool report mutation must apply")
        changed = copy.deepcopy(spec); changed["objects"][index]["object"]["Frame"]["paddr"] = address+0x100000000
        try: verify(system, changed, changed_report, hardware_config=True, dma_domain=True, local_memory=local_memory, uc_irq=uc_irq)
        except ValueError as error:
            require("contiguous private pool" in str(error), "pool mutation must reach contiguity check")
            rejected += 1
        else: raise ValueError("noncontiguous pool accepted")
    if uc_irq:
        tree = ET.parse(system); tree.find("protection_domain/irq[@id='2']").set("trigger", "level")
        with tempfile.NamedTemporaryFile(suffix=".system") as f:
            tree.write(f.name)
            try: verify(f.name, spec, report, hardware_config=True, dma_domain=True, local_memory=True, uc_irq=True)
            except ValueError: rejected += 1
            else: raise ValueError("wrong IPA XML trigger accepted")
        for field in ["trigger", "badge"]:
            changed = copy.deepcopy(spec)
            handler = next(q["handler"] for q in changed["irqs"] if q["irq"] == 343)
            irq = changed["objects"][handler]["object"]["ArmIrq"]
            if field == "trigger": irq["extra"]["trigger"] = 0
            else: irq["slots"][0]["cap"]["Notification"]["badge"] = 1
            try: verify(system, changed, report, hardware_config=True, dma_domain=True, local_memory=True, uc_irq=True)
            except ValueError: rejected += 1
            else: raise ValueError("wrong IPA CapDL " + field + " accepted")
    return rejected

if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--system", type=Path, default=Path(__file__).with_name("native-startup-preflight.system"))
    p.add_argument("--capdl", type=Path, required=True); p.add_argument("--report", type=Path, required=True)
    p.add_argument("--dt", type=Path, action="append", default=[]); p.add_argument("--output", type=Path, required=True)
    p.add_argument("--profile", choices=["preflight", "hardware-config", "dma-domain", "local-memory", "buffers", "resources", "irq"], default="preflight")
    args = p.parse_args(); spec = json.loads(args.capdl.read_text()); report = args.report.read_text()
    result = audit(args.system, spec, report, args.dt, args.profile != "preflight", args.profile in ["dma-domain", "local-memory", "buffers", "resources", "irq"], args.profile in ["local-memory", "buffers", "resources", "irq"], args.profile == "irq")
    result["mutations_rejected"] = mutations(args.system, spec, report, args.profile != "preflight", args.profile in ["dma-domain", "local-memory", "buffers", "resources", "irq"], args.profile in ["local-memory", "buffers", "resources", "irq"], args.profile == "irq")
    result["cpu_buffer_preparation"] = args.profile in ["buffers", "resources", "irq"]
    result["resource_limits"] = args.profile in ["resources", "irq"]
    result["uc_irq_arm"] = args.profile == "irq"
    args.output.write_text(json.dumps(result, indent=2) + "\n"); print(json.dumps(result, indent=2))
