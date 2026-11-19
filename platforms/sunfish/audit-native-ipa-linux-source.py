#!/usr/bin/env python3
"""Resolve the first native IPA stage against the exact successful kernel."""
from pathlib import Path
from collections import deque
import argparse
import hashlib
import json
import re
import tarfile

ROOT = Path(__file__).resolve().parents[2]
PIN = "2267e248204e7e051e0f7355298949a6440de579"
ARCHIVE_SHA256 = "d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2"


def hardware_profile(source):
    """Derive the nine v4.2 primitive writes independently of the Rust values."""
    main = source["drivers/net/ipa/ipa_main.c"]
    regs = source["drivers/net/ipa/reg/ipa_reg-v4.2.c"]
    data = source["drivers/net/ipa/data/ipa_data-v4.2.c"]
    endpoint = source["drivers/net/ipa/ipa_endpoint.h"]
    def function(name):
        start = re.search(r"\b" + name + r"\([^;]*?\)\s*\{", main, re.S)
        if not start: raise ValueError("missing function " + name)
        at = start.end(); depth = 1
        for i in range(at, len(main)):
            depth += (main[i] == "{") - (main[i] == "}")
            if not depth: return main[at:i]
        raise ValueError("unclosed function")
    def contains(name, *pieces):
        body = function(name)
        for p in pieces:
            if p not in body: raise ValueError(name + ": missing " + p)
        return body
    order = ["bcr", "tx", "clkon", "comp", "qsb", "timing", "hashing", "dcd"]
    calls = re.findall(r"ipa_hardware_(?:config_([a-z]+)|(dcd)_config)\(ipa", function("ipa_hardware_config"))
    if [a or b for a, b in calls] != order: raise ValueError("primitive config order changed")
    contains("ipa_hardware_config_bcr", "val = data->backward_compat", "ipa->version >= IPA_VERSION_4_5")
    contains("ipa_hardware_config_tx", "version <= IPA_VERSION_4_0 || version >= IPA_VERSION_4_5", "val &= ~reg_bit(reg, PA_MASK_EN)")
    contains("ipa_hardware_config_clkon", "val = reg_bit(reg, CLKON_GLOBAL)", "val |= reg_bit(reg, GLOBAL_2X_CLK)")
    contains("ipa_hardware_config_comp", "ipa->version < IPA_VERSION_4_5", "val |= reg_bit(reg, GSI_MULTI_AXI_MASTERS_DIS)", "val |= reg_bit(reg, GSI_MULTI_INORDER_RD_DIS)", "val |= reg_bit(reg, GSI_MULTI_INORDER_WR_DIS)")
    contains("ipa_hardware_config_qsb", "GEN_QMB_0_MAX_WRITES, data0->max_writes", "GEN_QMB_0_MAX_READS, data0->max_reads", "data0->max_reads_beats")
    contains("ipa_hardware_config_counter", "ipa_aggr_granularity_val(IPA_AGGR_GRANULARITY)", "AGGR_GRANULARITY, granularity")
    contains("ipa_hardware_config_timing", "ipa->version < IPA_VERSION_4_5", "ipa_hardware_config_counter(ipa)")
    contains("ipa_hardware_config_hashing", "ipa->version != IPA_VERSION_4_2", "iowrite32(0,")
    contains("ipa_hardware_dcd_config", "ipa_idle_indication_cfg(ipa, 256, false)")
    contains("ipa_idle_indication_cfg", "ENTER_IDLE_DEBOUNCE_THRESH", "if (const_non_idle_enable)")
    def offset(name):
        return int(re.search(r"REG(?:_FIELDS)?\(" + name + r",\s*\w+,\s*(0x[0-9a-f]+)\)", regs)[1], 16)
    def mask(register, field):
        body = re.search(r"reg_" + register + r"_fmask\[\] = \{(.*?)\n\};", regs, re.S)[1]
        value = re.search(r"\[" + field + r"\]\s*=\s*(BIT\(\d+\)|GENMASK\(\d+,\s*\d+\))", body)[1]
        bits = list(map(int, re.findall(r"\d+", value)))
        return 1 << bits[0] if value.startswith("BIT") else ((1 << (bits[0]+1))-1) ^ ((1 << bits[1])-1)
    def encode(register, field, value):
        m = mask(register, field); shift = (m & -m).bit_length()-1
        if value << shift & ~m: raise ValueError("field overflow")
        return value << shift
    qsb = re.search(r"ipa_qsb_data\[\] = \{(.*?)\n\};", data, re.S)[1]
    if qsb.count("[IPA_QSB_MASTER_") != 1 or ".max_reads_beats" in qsb:
        raise ValueError("QSB master topology changed")
    if re.search(r"\.backward_compat\s*=", data): raise ValueError("nonzero backward compatibility")
    wr, rd = [int(re.search(r"\.max_" + n + r"\s*=\s*(\d+)", qsb)[1]) for n in ["writes", "reads"]]
    us = int(re.search(r"#define IPA_AGGR_GRANULARITY\s+(\d+)", endpoint)[1])
    hz = int(re.search(r"#define TIMER_FREQUENCY\s+(\d+)", main)[1])
    contains("ipa_aggr_granularity_val", "DIV_ROUND_CLOSEST(usec * TIMER_FREQUENCY, USEC_PER_SEC) - 1")
    granularity = (us * hz + 500000) // 1000000 - 1
    rows = [
        ("Bcr", "IPA_BCR", 0xffffffff, 0),
        ("Tx", "IPA_TX_CFG", mask("ipa_tx_cfg", "PA_MASK_EN"), 0),
        ("Clkon", "CLKON_CFG", 0xffffffff, mask("clkon_cfg", "CLKON_GLOBAL") | mask("clkon_cfg", "GLOBAL_2X_CLK")),
        ("Comp", "COMP_CFG", 0, sum(mask("comp_cfg", f) for f in ["GSI_MULTI_AXI_MASTERS_DIS", "GSI_MULTI_INORDER_RD_DIS", "GSI_MULTI_INORDER_WR_DIS"])),
        ("QsbWrites", "QSB_MAX_WRITES", 0xffffffff, encode("qsb_max_writes", "GEN_QMB_0_MAX_WRITES", wr)),
        ("QsbReads", "QSB_MAX_READS", 0xffffffff, encode("qsb_max_reads", "GEN_QMB_0_MAX_READS", rd)),
        ("Counter", "COUNTER_CFG", 0xffffffff, encode("counter_cfg", "AGGR_GRANULARITY", granularity)),
        ("Hash", "FILT_ROUT_HASH_EN", 0xffffffff, 0),
        ("Idle", "IDLE_INDICATION_CFG", 0xffffffff, encode("idle_indication_cfg", "ENTER_IDLE_DEBOUNCE_THRESH", 256)),
    ]
    derived = [(name, offset(reg), clear, value) for name, reg, clear, value in rows]
    native = (ROOT / "rust/src/sunfish_ipa_linux_config.rs").read_text()
    actual = [(name, int(off,16), int(clear,16), int(value,16)) for name,off,clear,value in re.findall(
        r"Spec\s*\{\s*register:\s*Register::(\w+),\s*offset:\s*(0x[0-9a-f]+),\s*clear:\s*(0x[0-9a-f]+),\s*set:\s*(0x[0-9a-f]+),?\s*\}", native)]
    if actual != derived: raise ValueError("native primitive IPA profile differs from pinned source")
    return {"writes": [{"register": n, "offset": hex(o), "clear": hex(c), "set": hex(v)} for n,o,c,v in derived],
            "granularity_us": us, "timer_hz": hz, "scope": "ipa_hardware_config only; remaining ipa_config stages and PAS/setup not executed"}


def hardware_mutations(source):
    cases = [
        ("drivers/net/ipa/reg/ipa_reg-v4.2.c", r"(\[PA_MASK_EN\]\s*=\s*BIT\()12", r"\g<1>13"),
        ("drivers/net/ipa/reg/ipa_reg-v4.2.c", r"counter_cfg, 0x000001f0", "counter_cfg, 0x000001f4"),
        ("drivers/net/ipa/data/ipa_data-v4.2.c", r"(\.max_reads\s*=\s*)12", r"\g<1>13"),
        ("drivers/net/ipa/ipa_main.c", r"ipa_hardware_config_tx\(ipa\);", "ipa_hardware_config_hashing(ipa);"),
    ]
    for path, pattern, replacement in cases:
        changed = dict(source)
        changed[path], n = re.subn(pattern, replacement, source[path], count=1)
        if n != 1: raise ValueError("mutation did not apply: " + pattern)
        try: hardware_profile(changed)
        except ValueError: pass
        else: raise ValueError("hardware source mutation accepted: " + pattern)
    return len(cases)


def memory_profile(source):
    """Independently derive the v4.2 SRAM aperture, regions and canary order."""
    data = source["drivers/net/ipa/data/ipa_data-v4.2.c"]
    mem = source["drivers/net/ipa/ipa_mem.c"]
    regs = source["drivers/net/ipa/reg/ipa_reg-v4.2.c"]
    dt = source["arch/arm64/boot/dts/qcom/sm7150.dtsi"]
    body = re.search(r"ipa_mem_local_data\[\] = \{(.*?)\n\};", data, re.S)[1]
    regions = []
    for block in re.findall(r"\{(.*?)\}", body, re.S):
        name = re.search(r"\.id\s*=\s*IPA_MEM_(\w+)", block)[1]
        offset, size, count = [int(re.search(r"\."+field+r"\s*=\s*(0x[0-9a-f]+|\d+)", block)[1], 0)
                               for field in ["offset", "size", "canary_count"]]
        regions.append((name, offset, size, count))
    native = (ROOT / "rust/src/sunfish_ipa_linux_memory.rs").read_text()
    actual = [(name, int(offset,16), int(size,0), int(count)) for name,offset,size,count in re.findall(
        r'Region\s*\{\s*name:\s*"(\w+)",\s*offset:\s*(0x[0-9a-f]+),\s*size:\s*(0x[0-9a-f]+|\d+),\s*canary_count:\s*(\d+),?\s*\}', native)]
    if actual != regions: raise ValueError("native local-memory regions differ from pinned source")
    ipa_node = re.search(r"ipa: ipa@1e40000 \{(.*?)\n\t\t\};", dt, re.S)[1]
    spans = [(int(pa,16),int(size,16)) for pa,size in re.findall(r"<0x0 (0x[0-9a-f]+) 0x0 (0x[0-9a-f]+)>", ipa_node)]
    if spans != [(0x01e40000,0x7000),(0x01e47000,0x2000),(0x01e04000,0x2c000)]:
        raise ValueError("pinned IPA register/shared/GSI apertures changed")
    if int(re.search(r"SHARED_PHYSICAL: u64 = (0x[0-9a-f]+)",native)[1],16) != spans[1][0]:
        raise ValueError("native shared-memory physical base differs")
    if int(re.search(r"SHARED_BYTES: usize = (0x[0-9a-f]+)",native)[1],16) != spans[1][1]:
        raise ValueError("native shared-memory aperture differs")
    for text in ["ipa->mem_offset = 8 * reg_decode(reg, MEM_BADDR, val);",
                 "mem_size = 8 * reg_decode(reg, MEM_SIZE, val);",
                 "canary = ipa->mem_virt + ipa->mem_offset + ipa->mem[i].offset;",
                 "*--canary = IPA_MEM_CANARY_VAL;", "while (--canary_count);"]:
        if text not in mem: raise ValueError("local-memory operation changed: " + text)
    for field, high, low in [("MEM_SIZE",15,0),("MEM_BADDR",31,16)]:
        if not re.search(r"\["+field+r"\]\s*=\s*GENMASK\("+str(high)+r",\s*"+str(low)+r"\)",regs):
            raise ValueError("shared-memory field changed: " + field)
    value = int(re.search(r"#define IPA_MEM_CANARY_VAL\s+cpu_to_le32\((0x[0-9a-f]+)\)",mem)[1],16)
    if int(re.search(r"CANARY_VALUE: u32 = (0x[0-9a-f]+)",native)[1],16) != value:
        raise ValueError("canary pattern differs")
    offsets = [offset - 4 * word for _,offset,_,count in regions for word in range(1,count+1)]
    if len(offsets) != 25 or len(set(offsets)) != 25 or any(x < 0 or x+4 > spans[1][1] for x in offsets):
        raise ValueError("canary geometry changed")
    return {"shared_physical":hex(spans[1][0]),"mapped_bytes":spans[1][1],"regions":[
        {"name":n,"offset":hex(o),"size":hex(s),"canary_count":c} for n,o,s,c in regions],
        "canary_value":hex(value),"canary_offsets":[hex(x) for x in offsets],
        "measured_shared_mem_size":"0x415","reported_bytes":0x415*8,"effective_bytes":spans[1][1],
        "scope":"CPU canary portion only; no DMA zero-buffer allocator, IMEM/SMEM mapping or ipa_mem_setup commands"}


def memory_mutations(source):
    cases = [
        ("drivers/net/ipa/data/ipa_data-v4.2.c", r"(\.canary_count\s*=\s*)2", r"\g<1>1"),
        ("drivers/net/ipa/ipa_mem.c", r"\*--canary = IPA_MEM_CANARY_VAL", "*canary++ = IPA_MEM_CANARY_VAL"),
        ("drivers/net/ipa/reg/ipa_reg-v4.2.c", r"(\[MEM_SIZE\]\s*=\s*GENMASK\()15", r"\g<1>14"),
        ("arch/arm64/boot/dts/qcom/sm7150.dtsi", r"0x01e47000 0x0 0x2000", "0x01e47000 0x0 0x3000"),
    ]
    for path,pattern,replacement in cases:
        changed=dict(source);changed[path],count=re.subn(pattern,replacement,source[path],count=1)
        if count != 1: raise ValueError("memory mutation did not apply: " + pattern)
        try: memory_profile(changed)
        except ValueError: pass
        else: raise ValueError("memory source mutation accepted: " + pattern)
    return len(cases)


def buffer_profile(source):
    """Derive DMA-memory requirements without borrowing the legacy profile."""
    data=source["drivers/net/ipa/data/ipa_data-v4.2.c"]
    gsi=source["drivers/net/ipa/gsi.c"]
    private=source["drivers/net/ipa/gsi_private.h"]
    trans=source["drivers/net/ipa/gsi_trans.c"]
    cmd=source["drivers/net/ipa/ipa_cmd.c"]
    table=source["drivers/net/ipa/ipa_table.c"]
    endpoint=source["drivers/net/ipa/ipa_endpoint.c"]
    mem_header=source["drivers/net/ipa/ipa_mem.h"]
    native=(ROOT/"rust/src/sunfish_ipa_linux_buffers.rs").read_text()
    def constant(name):
        return int(re.search(name+r": (?:usize|u64) = (0x[0-9a-f]+|\d+)",native)[1],0)
    blocks=[]
    for match in re.finditer(r"\[(IPA_ENDPOINT_\w+)\] = \{",data):
        start=match.end();depth=1
        for i in range(start,len(data)):
            depth += (data[i]=='{')-(data[i]=='}')
            if depth==0:break
        else:raise ValueError("endpoint block not closed")
        blocks.append((match[1].removeprefix("IPA_ENDPOINT_"),data[start:i]))
    channels=[];filtered=0
    for name,body in blocks:
        endpoint_id=int(re.search(r"\.endpoint_id\s*=\s*(\d+)",body)[1])
        if re.search(r"\.filter_support\s*=\s*true",body):filtered |= 1 << endpoint_id
        if not re.search(r"\.ee_id\s*=\s*GSI_EE_AP",body):continue
        channel_id=int(re.search(r"\.channel_id\s*=\s*(\d+)",body)[1])
        counts=[int(re.search(r"\."+field+r"\s*=\s*(\d+)",body)[1]) for field in ["tre_count","event_count","tlv_count"]]
        channels.append((name,channel_id,endpoint_id,*counts))
    actual=[(name,*map(int,values)) for name,*values in re.findall(
        r'Channel\s*\{\s*name:\s*"(\w+)",\s*channel_id:\s*(\d+),\s*endpoint_id:\s*(\d+),\s*tre_count:\s*(\d+),\s*event_count:\s*(\d+),\s*tlv_count:\s*(\d+),?\s*\}',native)]
    if actual!=channels:raise ValueError("native four-AP-channel buffer profile differs")
    element=int(re.search(r"#define GSI_RING_ELEMENT_SIZE\s+(\d+)",private)[1])
    if element!=constant("RING_ELEMENT_BYTES"):raise ValueError("ring element size differs")
    for text in ["u32 size = count * GSI_RING_ELEMENT_SIZE;", "return channel->tre_count - (channel->trans_tre_max - 1);"]:
        if text not in gsi:raise ValueError("GSI memory requirement changed: "+text)
    if "gsi_channel_evt_ring_init(channel)" not in gsi or "ipa_cmd_pool_init(channel, tre_max)" not in gsi:
        raise ValueError("GSI AP buffer allocation lifecycle changed")
    # Derive the union's LP64 layout from the actual fixed-width declarations,
    # rejecting unknown fields/types instead of assuming its rounded size.
    union=re.search(r"union ipa_cmd_payload \{(.*?)\n\};",cmd,re.S)[1]
    types={"__le64":(8,8),"__le32":(4,4),"__le16":(2,2),"u8":(1,1)}
    sizes=[]
    for name in re.findall(r"struct (\w+) \w+;",union):
        body=re.search(r"struct "+name+r" \{(.*?)\n\};",cmd,re.S)[1]
        body=re.sub(r"/\*.*?\*/","",body,flags=re.S)
        pattern=r"\b(__le64|__le32|__le16|u8)\s+\w+(?:\[(\d+)\])?\s*;"
        fields=re.findall(pattern,body)
        if re.sub(pattern,"",body).strip():raise ValueError("unknown command payload declaration")
        offset=0;alignment=1
        for kind,count in fields:
            size,align=types[kind];offset=(offset+align-1)//align*align+size*int(count or 1);alignment=max(alignment,align)
        sizes.append((offset+alignment-1)//alignment*alignment)
    payload=max(sizes);stride=1 << (payload-1).bit_length()
    if stride!=constant("COMMAND_ELEMENT_BYTES"):raise ValueError("command payload stride differs")
    for text in ["size = __roundup_pow_of_two(size);", "total_size = (count + max_alloc - 1) * size;", "total_size = PAGE_SIZE << get_order(total_size);"]:
        if text not in trans:raise ValueError("command DMA pool allocation changed: "+text)
    command=next(c for c in channels if c[0]=="AP_COMMAND_TX")
    tre_max=command[3]-(command[5]-1)
    command_bytes=(tre_max+command[5]-1)*stride
    command_bytes=max(4096,1 << (command_bytes-1).bit_length())
    if command_bytes!=constant("COMMAND_POOL_BYTES"):raise ValueError("command pool size differs")
    if not re.search(r"#define IPA_MEM_MAX\s+\(\s*2\s*\*\s*PAGE_SIZE\)",mem_header):
        raise ValueError("DMA zero-source allocation size changed")
    config_path=ROOT/".cache/sunfish-mobian-comparison/artifacts/kernel.config"
    configuration=config_path.read_text()
    if "CONFIG_ARM64_4K_PAGES=y" not in configuration:raise ValueError("comparison page granule differs")
    if constant("ZERO_BYTES")!=8192 or constant("PAGE_BYTES")!=4096:raise ValueError("native zero buffer/page size differs")
    regions=memory_profile(source)["regions"]
    filter_size=int(next(r["size"] for r in regions if r["name"]=="V4_FILTER"),16)
    route_size=int(next(r["size"] for r in regions if r["name"]=="V4_ROUTE"),16)
    entries=max(filter_size//8-1,route_size//8)
    if constant("TABLE_ENTRIES")!=entries or constant("FILTER_BITMAP")!=filtered<<1:
        raise ValueError("empty table count/bitmap differs")
    for text in ["if (data->endpoint.filter_support)","filtered |= BIT(data->endpoint_id);"]:
        if text not in endpoint:raise ValueError("endpoint filter bitmap lifecycle changed")
    for text in ["count = max_t(u32, ipa->filter_count, ipa->route_count);", "size = IPA_ZERO_RULE_SIZE + (1 + count) * sizeof(__le64);", "*virt++ = cpu_to_le64(ipa->filtered << 1);", "le_addr = cpu_to_le64(addr);", "*virt++ = le_addr;"]:
        if text not in table:raise ValueError("empty table initialization changed: "+text)
    return {"channels":[dict(zip(["name","channel_id","endpoint_id","tre_count","event_count","tlv_count"],c)) for c in channels],
            "ring_element_bytes":element,"ring_bytes":sum((c[3]+c[4])*element for c in channels),
            "command_payload_abi_bytes":payload,"command_stride":stride,"command_tre_max":tre_max,"command_pool_bytes":command_bytes,
            "zero_bytes":8192,"table_entries":entries,"table_payload_bytes":8+(entries+1)*8,"filter_bitmap":hex(filtered<<1),
            "comparison_config_sha256":hashlib.sha256(configuration.encode()).hexdigest(),
            "scope":"coherent-memory requirements and CPU initialization only; no hardware event IDs, commands, RX packet buffers or DMA proof"}


def buffer_mutations(source):
    cases=[
        ("drivers/net/ipa/data/ipa_data-v4.2.c",r"(\.event_count\s*=\s*)256",r"\g<1>512"),
        ("drivers/net/ipa/gsi_private.h",r"(#define GSI_RING_ELEMENT_SIZE\s+)16",r"\g<1>32"),
        ("drivers/net/ipa/ipa_cmd.c",r"__le64\s+nhash_rules_addr;","__le64 nhash_rules_addr[3];"),
        ("drivers/net/ipa/data/ipa_data-v4.2.c",r"(\.filter_support\s*=\s*)true",r"\g<1>false"),
        ("drivers/net/ipa/ipa_mem.h",r"(#define IPA_MEM_MAX\s+\()2",r"\g<1>3"),
    ]
    for path,pattern,replacement in cases:
        changed=dict(source);changed[path],count=re.subn(pattern,replacement,source[path],count=1)
        if count!=1:raise ValueError("buffer source mutation did not apply")
        try:buffer_profile(changed)
        except ValueError:pass
        else:raise ValueError("buffer source mutation accepted: "+pattern)
    return len(cases)


def resource_profile(source):
    """Derive all seven group-0 limits and eight endpoint directions."""
    data = source["drivers/net/ipa/data/ipa_data-v4.2.c"]
    regs = source["drivers/net/ipa/reg/ipa_reg-v4.2.c"]
    resource = source["drivers/net/ipa/ipa_resource.c"]
    table = source["drivers/net/ipa/ipa_table.c"]
    endpoint = source["drivers/net/ipa/ipa_endpoint.c"]
    native = (ROOT / "rust/src/sunfish_ipa_linux_resources.rs").read_text()
    def enum_values(name):
        body = re.search(r"enum " + name + r" \{(.*?)\n\};", data, re.S)[1]
        body = re.sub(r"/\*.*?\*/", "", body, flags=re.S)
        values = {}; current = -1
        for entry in body.split(","):
            if not entry.strip(): continue
            match = re.fullmatch(r"\s*(\w+)\s*(?:=\s*(\d+))?\s*", entry)
            if not match: raise ValueError("unsupported resource enum")
            current = int(match[2]) if match[2] else current + 1
            values[match[1]] = current
        return values
    groups = enum_values("ipa_rsrc_group_id")
    types = enum_values("ipa_resource_type")
    if groups["IPA_RSRC_GROUP_SRC_COUNT"] != 1 or groups["IPA_RSRC_GROUP_DST_COUNT"] != 1:
        raise ValueError("resource group count changed")
    for token in ["ylimits = group_count == 1 ? NULL", "if (group_count < 3)",
                  "val = reg_encode(reg, X_MIN_LIM, xlimits->min);",
                  "val |= reg_encode(reg, X_MAX_LIM, xlimits->max);",
                  "iowrite32(val, ipa->reg_virt + reg_n_offset(reg, resource_type));",
                  "for (i = 0; i < data->resource_src_count; i++)",
                  "for (i = 0; i < data->resource_dst_count; i++)"]:
        if token not in resource: raise ValueError("resource write lifecycle changed: " + token)
    if resource.index("ipa_resource_config_src(ipa, i, data)") > resource.index("ipa_resource_config_dst(ipa, i, data)"):
        raise ValueError("resource direction order changed")
    derived = []; limits = []
    for direction in ["src", "dst"]:
        regname = direction + "_rsrc_grp_01_rsrc_type"
        offset, stride = [int(v, 16) for v in re.search(
            r"REG_STRIDE_FIELDS\(\w+, " + regname + r",\s*(0x[0-9a-f]+),\s*(0x[0-9a-f]+)\)", regs).groups()]
        fields = re.search(r"reg_" + regname + r"_fmask\[\] = \{(.*?)\n\};", regs, re.S)[1]
        shifts = {}
        for field in ["X_MIN_LIM", "X_MAX_LIM", "Y_MIN_LIM", "Y_MAX_LIM"]:
            high, low = map(int, re.search(r"\[" + field + r"\]\s*=\s*GENMASK\((\d+),\s*(\d+)\)", fields).groups())
            if high - low != 5: raise ValueError("resource field width changed")
            shifts[field] = low
        block = re.search(r"ipa_resource_" + direction + r"\[\] = \{(.*?)\n\};", data, re.S)[1]
        entries = re.findall(r"\[(IPA_RESOURCE_TYPE_\w+)\]\s*=\s*\{\s*\.limits\[(\w+)\]\s*=\s*\{\s*\.min\s*=\s*(\d+),\s*\.max\s*=\s*(\d+),\s*\},\s*\},", block)
        if len(entries) != block.count(".limits["): raise ValueError("unsupported resource group layout")
        for name, group, minimum, maximum in entries:
            minimum, maximum = int(minimum), int(maximum)
            if groups[group] != 0 or not 0 <= minimum <= maximum <= 63:
                raise ValueError("resource limits outside group 0 / six-bit range")
            value = (minimum << shifts["X_MIN_LIM"]) | (maximum << shifts["X_MAX_LIM"])
            derived.append((offset + stride * types[name], value))
            limits.append({"name": name, "group": groups[group], "min": minimum, "max": maximum})
    actual = [(int(offset,16), int(value,16)) for offset,value in re.findall(
        r"Spec\s*\{\s*register:\s*Register::\w+,\s*offset:\s*(0x[0-9a-f]+),\s*value:\s*(0x[0-9a-f]+),?\s*\}", native)]
    if derived != actual or len(derived) != 7: raise ValueError("native resource limit writes differ")
    definitions = []
    for match in re.finditer(r"\[IPA_ENDPOINT_\w+\]\s*=\s*\{", data):
        start = match.end(); depth = 1
        for end in range(start, len(data)):
            depth += (data[end] == "{") - (data[end] == "}")
            if not depth: break
        body = data[start:end]
        definitions.append((int(re.search(r"\.endpoint_id\s*=\s*(\d+)",body)[1]),
                            re.search(r"\.toward_ipa\s*=\s*(true|false)",body)[1] == "true"))
    native_definitions = [(int(i), direction == "true") for i,direction in re.findall(
        r"Endpoint\s*\{\s*id:\s*(\d+),\s*toward_ipa:\s*(true|false),?\s*\}",native)]
    if definitions != native_definitions: raise ValueError("native endpoint directions differ")
    for field, expected in [("MAX_CONS_PIPES",(12,8)),("MAX_PROD_PIPES",(20,16)),("PROD_LOWEST",(27,24))]:
        actual = tuple(map(int,re.search(r"\["+field+r"\]\s*=\s*GENMASK\((\d+),\s*(\d+)\)",regs).groups()))
        if actual != expected: raise ValueError("FLAVOR endpoint fields changed")
    for token in ["tx_count = reg_decode(reg, MAX_CONS_PIPES, val);", "rx_count = reg_decode(reg, MAX_PROD_PIPES, val);",
                  "rx_base = reg_decode(reg, PROD_LOWEST, val);", "if (endpoint_id >= limit)",
                  "if (!test_bit(endpoint_id, ipa->available))", "if (endpoint_id < tx_count)",
                  "else if (endpoint_id >= rx_base)"]:
        if token not in endpoint: raise ValueError("endpoint census changed: " + token)
    if not re.search(r"bool ipa_table_hash_support\(struct ipa \*ipa\)\s*\{\s*return ipa->version != IPA_VERSION_4_2;\s*\}",table):
        raise ValueError("v4.2 hash exclusion changed")
    for function in ["ipa_filter_config", "ipa_route_config"]:
        body = re.search(r"static void " + function + r"\(.*?\n\}", table, re.S)[0]
        if "if (!ipa_table_hash_support(ipa))\n\t\treturn;" not in body:
            raise ValueError("hash table configuration is no longer a v4.2 no-op")
    return {"limits": limits, "writes": [{"offset":hex(o),"value":hex(v)} for o,v in derived],
            "endpoints": [{"id":i,"toward_ipa":d} for i,d in definitions], "hash_table_configuration":"no writes on v4.2",
            "scope":"endpoint availability and resource limits only; IRQ/UC configuration intentionally deferred; no endpoint programming, DMA, GSI or PAS"}


def resource_mutations(source):
    cases = [
        ("drivers/net/ipa/data/ipa_data-v4.2.c", r"(\.min = 3,\s*\.max = )63", r"\g<1>62"),
        ("drivers/net/ipa/reg/ipa_reg-v4.2.c", r"0x00000400, 0x0020", "0x00000400, 0x0040"),
        ("drivers/net/ipa/reg/ipa_reg-v4.2.c", r"(\[X_MAX_LIM\]\s*=\s*GENMASK\()13, 8", r"\g<1>14, 9"),
        ("drivers/net/ipa/data/ipa_data-v4.2.c", r"(\.endpoint_id\s*=\s*)6", r"\g<1>7"),
        ("drivers/net/ipa/ipa_table.c", r"return ipa->version != IPA_VERSION_4_2;", "return true;"),
    ]
    for path, pattern, replacement in cases:
        changed = dict(source); changed[path], count = re.subn(pattern,replacement,source[path],count=1)
        if count != 1: raise ValueError("resource source mutation did not apply")
        try: resource_profile(changed)
        except ValueError: pass
        else: raise ValueError("resource source mutation accepted: " + pattern)
    return len(cases)


def irq_profile(source):
    """Bind the narrow IRQ arm test to the actual v4.2/AP source definitions."""
    regs = source["drivers/net/ipa/reg/ipa_reg-v4.2.c"]
    header = source["drivers/net/ipa/ipa_reg.h"]
    interrupt = source["drivers/net/ipa/ipa_interrupt.c"]
    uc = source["drivers/net/ipa/ipa_uc.c"]
    dt = source["arch/arm64/boot/dts/qcom/sm7150.dtsi"]
    native = (ROOT/"rust/src/sunfish_ipa_linux_irq.rs").read_text()
    entry = (ROOT/"rust/src/sunfish_native_startup_preflight.rs").read_text()
    def native_constant(name):
        return int(re.search(r"pub const " + name + r": \w+ = (0x[0-9a-f]+|\d+);",native)[1],0)
    bits = {name:int(re.search(r"\bIPA_IRQ_UC_" + str(i) + r"\s*=\s*(0x[0-9a-f]+)",header)[1],16)
            for name,i in [("EVENT_MASK",0),("RESPONSE_MASK",1)]}
    for name,bit in bits.items():
        actual = int(re.search(r"pub const " + name + r": u32 = 1 << (\d+);",native)[1])
        if actual != bit: raise ValueError("native UC mask differs")
    disabled_bits = [int(re.search(r"\b"+name+r"\s*=\s*(0x[0-9a-f]+)",header)[1],16)
                     for name in ["IPA_IRQ_UC_RX_CMD_Q_NOT_FULL","IPA_IRQ_BAM_GSI_IDLE"]]
    native_disabled = re.search(r"pub const ALLOWED_DISABLED_STATUS: u32 = \(1 << (\d+)\) \| \(1 << (\d+)\);",native)
    if not native_disabled or list(map(int,native_disabled.groups())) != disabled_bits:
        raise ValueError("disabled status exception differs from source bit definitions")
    allowed_status = sum(1<<bit for bit in disabled_bits)
    if allowed_status & sum(1<<bit for bit in bits.values()): raise ValueError("disabled status overlaps enabled indications")
    for name,regname in [("ENABLE_OFFSET","IPA_IRQ_EN"),("STATUS_OFFSET","IPA_IRQ_STTS")]:
        value = int(re.search(r"REG\("+regname+r", \w+, (0x[0-9a-f]+) \+ 0x1000 \* GSI_EE_AP\)",regs)[1],16)
        if native_constant(name) != value: raise ValueError("native IRQ register offset differs")
    # GSI_EE_AP is zero in the source ABI; do not silently reuse another EE.
    gsi_header = source["drivers/net/ipa/ipa_version.h"]
    ap_ee = re.search(r"\bGSI_EE_AP\s*=\s*(0x[0-9a-f]+|\d+),",gsi_header)
    if not ap_ee or int(ap_ee[1],0) != 0: raise ValueError("AP EE number changed")
    node = re.search(r"ipa: ipa@1e40000 \{(.*?)\n\t\t\};",dt,re.S)[1]
    irqs = re.findall(r"<&intc GIC_SPI (\d+) (IRQ_TYPE_\w+)>",node)
    if len(irqs) != 2 or irqs[0][1] != "IRQ_TYPE_EDGE_RISING": raise ValueError("IPA IRQ type changed")
    physical = int(irqs[0][0])+32
    if native_constant("IRQ") != physical or native_constant("CHANNEL") != 2:
        raise ValueError("native IRQ identity differs")
    for token in ["interrupt->enabled = 0;", "iowrite32(0, ipa->reg_virt + reg_offset(reg));",
                  "interrupt->enabled |= BIT(ipa_irq);", "iowrite32(ipa->interrupt->enabled, ipa->reg_virt + reg_offset(reg));",
                  "while ((mask = pending & enabled))"]:
        if token not in interrupt: raise ValueError("IRQ configuration lifecycle changed")
    config = re.search(r"void ipa_uc_config\(.*?\n\}",uc,re.S)[0]
    if re.findall(r"ipa_interrupt_enable\(ipa, (IPA_IRQ_UC_\d)\)",config) != ["IPA_IRQ_UC_0","IPA_IRQ_UC_1"]:
        raise ValueError("UC interrupt enable order changed")
    # Derive actual C field positions, including reserved bytes and alignment.
    body = re.search(r"struct ipa_uc_mem_area \{(.*?)\n\};",uc,re.S)[1]
    body = re.sub(r"/\*.*?\*/","",body,flags=re.S)
    pattern = r"\b(u8|__le16|__le32)\s+(\w+)(?:\[(\d+)\])?\s*;"
    if re.sub(pattern,"",body).strip(): raise ValueError("unknown UC shared layout")
    position=0;fields={};sizes={"u8":1,"__le16":2,"__le32":4}
    for kind,name,count in re.findall(pattern,body):
        size=sizes[kind];position=(position+size-1)//size*size;fields[name]=position;position+=size*int(count or 1)
    for field,name in [("event","Event"),("response","Response")]:
        actual = int(re.search(r"Self::"+name+r" => (\d+)",native)[1])
        if actual != fields[field]: raise ValueError("UC defined-byte offset differs")
    # This probe deliberately contains notifications instead of claiming a
    # production UC handler; source inspection must still show a real ACK
    # binding before nonzero enables and no framework startup re-ACK.
    for token in ["Channel::new(irq::CHANNEL as usize)", ".irq_ack()", "fn run(&mut self)",
                  "sel4_microkit::ipc::recv()", "contain_irq(self)", "field.offset()).read_volatile()"]:
        if token not in entry: raise ValueError("native IRQ binding/containment path missing: "+token)
    return {"physical_irq":physical,"channel":2,"trigger":"edge",
            "enable_offset":hex(native_constant("ENABLE_OFFSET")),"status_offset":hex(native_constant("STATUS_OFFSET")),
            "enable_values":[0,1<<bits["EVENT_MASK"],(1<<bits["EVENT_MASK"])|(1<<bits["RESPONSE_MASK"])],
            "allowed_disabled_status_mask":hex(allowed_status),
            "allowed_disabled_status_bits":dict(zip(["UC_RX_CMD_Q_NOT_FULL","BAM_GSI_IDLE"],disabled_bits)),
            "shared_defined_byte_offsets":{name:fields[name] for name in ["event","response"]},
            "scope":"arm/readback only; any notification masks and stops without clearing pending status or ACKing delivery; no UC commands, firmware or interrupt-delivery proof"}


def irq_mutations(source):
    cases = [
        ("drivers/net/ipa/reg/ipa_reg-v4.2.c",r"ipa_irq_en, 0x0000300c","ipa_irq_en, 0x0000302c"),
        ("drivers/net/ipa/ipa_reg.h",r"(IPA_IRQ_UC_0\s*=\s*)0x2",r"\g<1>0x4"),
        ("arch/arm64/boot/dts/qcom/sm7150.dtsi",r"GIC_SPI 311 IRQ_TYPE_EDGE_RISING","GIC_SPI 311 IRQ_TYPE_LEVEL_HIGH"),
        ("drivers/net/ipa/ipa_uc.c",r"u8 reserved0\[3\]","u8 reserved0[4]"),
        ("drivers/net/ipa/ipa_uc.c",r"ipa_interrupt_enable\(ipa, IPA_IRQ_UC_0\);","ipa_interrupt_enable(ipa, IPA_IRQ_UC_1);"),
        ("drivers/net/ipa/ipa_reg.h",r"(IPA_IRQ_UC_RX_CMD_Q_NOT_FULL\s*=\s*)0x7",r"\g<1>0x6"),
    ]
    for path,pattern,replacement in cases:
        changed=dict(source);changed[path],count=re.subn(pattern,replacement,source[path],count=1)
        if count!=1: raise ValueError("IRQ source mutation did not apply: "+pattern)
        try: irq_profile(changed)
        except ValueError: pass
        else: raise ValueError("IRQ source mutation accepted: "+pattern)
    return len(cases)


def shared_memory_profile(source):
    """Audit the still-missing pre-PAS memory ownership/mapping dependencies.

    This describes source requirements only. It neither inspects live SMEM nor
    allocates an item, constructs an SMMU mapping or grants device authority.
    """
    data = source["drivers/net/ipa/data/ipa_data-v4.2.c"]
    mem = source["drivers/net/ipa/ipa_mem.c"]
    main = source["drivers/net/ipa/ipa_main.c"]
    smem = source["drivers/soc/qcom/smem.c"]
    lock = source["drivers/hwspinlock/qcom_hwspinlock.c"]
    dt = source["arch/arm64/boot/dts/qcom/sm7150.dtsi"]
    board = source["arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts"]
    def define(text, name):
        return int(re.search(r"#define " + name + r"\s+(0x[0-9a-f]+|\d+)\b", text)[1],0)
    def field(text, name):
        return int(re.search(r"\." + name + r"\s*=\s*(0x[0-9a-f]+|\d+)\b", text)[1],0)
    imem, imem_size, smem_size = [field(data,n) for n in ["imem_addr","imem_size","smem_size"]]
    host, item = [define(mem,n) for n in ["QCOM_SMEM_HOST_MODEM","SMEM_IPA_FILTER_TABLE"]]
    if (imem,imem_size,smem_size,host,item) != (0x146a8000,0x2000,0x2000,1,497):
        raise ValueError("Sunfish shared-memory target profile changed")
    ipa_node = re.search(r"ipa: ipa@1e40000 \{(.*?)\n\t\t\};",dt,re.S)[1]
    board_ipa = re.search(r"&ipa \{(.*?)\n\};",board,re.S)[1]
    if re.search(r"\bsram\s*=",ipa_node+board_ipa):
        raise ValueError("DTS now overrides legacy IPA IMEM location")
    for token in ['of_parse_phandle(dev->of_node, "sram", 0)',
                  "imem_base = mem_data->imem_addr;", "imem_size = mem_data->imem_size;",
                  "qcom_smem_alloc(QCOM_SMEM_HOST_MODEM, SMEM_IPA_FILTER_TABLE, size)",
                  "qcom_smem_get(QCOM_SMEM_HOST_MODEM, SMEM_IPA_FILTER_TABLE, &actual)",
                  "if (ret && actual != size)", "addr = qcom_smem_virt_to_phys(virt);",
                  "size = PAGE_ALIGN(size + addr - phys);", "iova = phys;",
                  "IOMMU_READ | IOMMU_WRITE"]:
        if token not in mem: raise ValueError("IPA shared-memory lifecycle changed: "+token)
    positions=[main.index(t) for t in ["ret = ipa_mem_init(ipa, pdev, data->mem_data);",
                                      "ret = ipa_config(ipa, data);", "ret = ipa_firmware_load(dev);"]]
    if positions != sorted(positions): raise ValueError("memory/config/PAS order changed")
    smem_node = re.search(r"\n\tsmem: smem \{(.*?)\n\t\};",dt,re.S)[1]
    if "memory-region = <&smem_mem>;" not in smem_node: raise ValueError("SMEM reserved binding changed")
    lock_index=int(re.search(r"hwlocks = <&tcsr_mutex (\d+)>;",smem_node)[1])
    smem_base, smem_bytes = [int(v,16) for v in re.search(
        r"smem_mem: smem@\w+ \{\s*reg = <0x0 (0x[0-9a-f]+) 0x0 (0x[0-9a-f]+)>;",dt).groups()]
    # Match the exact mutex compatible, offset and stride rather than borrowing
    # a different Qualcomm generation's register geometry.
    lock_node = re.search(r"tcsr_mutex: hwlock@\w+ \{(.*?)\n\t\t\};",dt,re.S)[1]
    if 'compatible = "qcom,tcsr-mutex";' not in lock_node: raise ValueError("mutex compatible changed")
    lock_base=int(re.search(r"reg = <0x0 (0x[0-9a-f]+)",lock_node)[1],16)
    lock_data=re.search(r"of_tcsr_mutex = \{(.*?)\n\};",lock,re.S)[1]
    lock_offset,lock_stride=field(lock_data,"offset"),field(lock_data,"stride")
    owner=define(lock,"QCOM_MUTEX_APPS_PROC_ID")
    if (smem_base,smem_bytes,lock_index,lock_base,lock_offset,lock_stride,owner) != (0x86000000,0x200000,3,0x01f40000,0,0x1000,1):
        raise ValueError("Sunfish SMEM/lock geometry changed")
    for token in ["hwspin_lock_timeout_irqsave(__smem->hwlock,", "hwspin_unlock_irqrestore(__smem->hwlock, &flags);",
                  "part = xa_load(&__smem->partitions, host);", "else if (__smem->global_partition.virt_base)",
                  "alloc_size = sizeof(*hdr) + ALIGN(size, 8);", "hdr->padding_hdr = 0;",
                  "le32_add_cpu(&phdr->offset_free_uncached, alloc_size);"]:
        if token not in smem: raise ValueError("SMEM allocation lifecycle changed: "+token)
    allocator=re.search(r"static int qcom_smem_alloc_private\(.*?\n\}",smem,re.S)[0]
    if not allocator.index("hdr->canary =") < allocator.index("wmb();") < allocator.index("le32_add_cpu"):
        raise ValueError("SMEM allocation publication order changed")
    # Derive the concrete v12 metadata ABI, without reading item payloads.
    layouts={};types={"u8":(1,1),"u16":(2,2),"__le16":(2,2),"__le32":(4,4)}
    symbols={"SMEM_ITEM_COUNT":define(smem,"SMEM_ITEM_COUNT")}
    def layout(name):
        if name in layouts:return layouts[name]
        body=re.search(r"struct "+name+r" \{(.*?)\n\};",smem,re.S)[1]
        body=re.sub(r"/\*.*?\*/","",body,flags=re.S)
        pattern=r"\b(struct \w+|u8|u16|__le16|__le32)\s+(\w+)(?:\[(\w*)\])?\s*;"
        declarations=list(re.finditer(pattern,body))
        if re.sub(pattern,"",body).strip():raise ValueError("unknown SMEM ABI field in "+name)
        position=0;alignment=1;fields={}
        for match in declarations:
            kind,field_name,count=match.groups()
            if kind.startswith("struct "):
                nested=layout(kind.split()[1]);size,align=nested["bytes"],nested["alignment"]
            else:size,align=types[kind]
            amount=1 if count is None else (0 if count=="" else (int(count) if count.isdecimal() else symbols[count]))
            position=(position+align-1)//align*align;fields[field_name]=position
            position+=size*amount;alignment=max(alignment,align)
        result={"bytes":(position+alignment-1)//alignment*alignment,"alignment":alignment,"fields":fields};layouts[name]=result;return result
    expected_sizes={"smem_header":8400,"smem_ptable":32,"smem_ptable_entry":48,"smem_partition_header":32,"smem_private_entry":16}
    for name,expected in expected_sizes.items():
        if layout(name)["bytes"]!=expected:raise ValueError("SMEM metadata ABI size changed: "+name)
    if layout("smem_partition_header")["fields"]["offset_free_uncached"]!=12:
        raise ValueError("SMEM publication field moved")
    if define(smem,"SMEM_GLOBAL_PART_VERSION")!=12:raise ValueError("SMEM partition version changed")
    pool=(ROOT/"rust/src/sunfish_ipa_linux_buffers.rs").read_text()
    pool_iova=int(re.search(r"POOL_IOVA: u64 = (0x[0-9a-f]+);",pool)[1],16)
    lpae=(ROOT/"rust/src/sunfish_smmu_lpae.rs").read_text()
    if "MappingSpansMultipleL1Entries" not in lpae:raise ValueError("re-audit the native multi-root table capability")
    roots=sorted({imem>>30,pool_iova>>30,smem_base>>30})
    return {"imem":{"physical":hex(imem),"bytes":imem_size,"iova":"identity","source":"legacy data; no DTS sram override"},
            "smem":{"host":host,"item":item,"requested_bytes":smem_size,"reserved_physical":hex(smem_base),"reserved_bytes":smem_bytes,
                    "physical":"must be resolved from a real allocated/retrieved item; no fixed item address",
                    "iova":"identity of the page-aligned physical span; 2 or 3 pages for an 8 KiB item"},
            "hardware_lock":{"index":lock_index,"physical":hex(lock_base+lock_offset+lock_stride*lock_index),"apps_owner":owner},
            "metadata_layouts":layouts,"allocation_publication":"header fields, write barrier, then uncached free offset; persistent until AP reboot",
            "required_root_indices":roots,"current_private_pool_root_index":pool_iova>>30,
            "native_gap":"the installed four-frame adapter and existing multi-L3 planner do not cover multiple 1 GiB roots; need owned level-2 tables for roots 0 and 2 plus SMEM ownership before PAS",
            "scope":"source requirements only; no allocation, live SMEM inspection, mapping, secure call or readiness proof"}


def shared_memory_mutations(source):
    cases=[
        ("drivers/net/ipa/data/ipa_data-v4.2.c",r"(\.imem_addr\s*=\s*)0x146a8000",r"\g<1>0x146b8000"),
        ("drivers/net/ipa/ipa_mem.c",r"(#define SMEM_IPA_FILTER_TABLE\s+)497",r"\g<1>496"),
        ("drivers/net/ipa/data/ipa_data-v4.2.c",r"(\.smem_size\s*=\s*)0x00002000",r"\g<1>0x00001000"),
        ("drivers/hwspinlock/qcom_hwspinlock.c",r"(of_tcsr_mutex = \{\s*\.offset = 0,\s*\.stride = )0x1000",r"\g<1>0x80"),
        ("drivers/soc/qcom/smem.c",r"le32_add_cpu\(&phdr->offset_free_uncached, alloc_size\);","/* publication removed */"),
        ("drivers/soc/qcom/smem.c",r"(struct smem_partition_header \{.*?__le32 reserved\[)3",r"\g<1>4"),
    ]
    for path,pattern,replacement in cases:
        changed=dict(source);changed[path],count=re.subn(pattern,replacement,source[path],count=1,flags=re.S)
        if count!=1:raise ValueError("shared-memory source mutation did not apply")
        try:shared_memory_profile(changed)
        except ValueError:pass
        else:raise ValueError("shared-memory source mutation accepted: "+pattern)
    return len(cases)


def census_source_profile(source):
    """Bind the metadata-only diagnostic's constants to the audited v12 ABI."""
    import xml.etree.ElementTree as ET
    requirements=shared_memory_profile(source)
    native=(ROOT/"platforms/sunfish/smem-directory-probe.c").read_text()
    layouts=requirements["metadata_layouts"]
    expected={"SMEM_BYTES":requirements["smem"]["reserved_bytes"],
              "SMEM_HEADER_INITIALIZED":layouts["smem_header"]["fields"]["initialized"],
              "SMEM_HEADER_RESERVED":layouts["smem_header"]["fields"]["reserved"],
              "SMEM_PTABLE_HEADER":layouts["smem_ptable"]["bytes"],
              "SMEM_PTABLE_ENTRY":layouts["smem_ptable_entry"]["bytes"],
              "SMEM_PARTITION_HEADER":layouts["smem_partition_header"]["bytes"],
              "SMEM_PRIVATE_ENTRY":layouts["smem_private_entry"]["bytes"],
              "HWLOCK_APPS_OWNER":requirements["hardware_lock"]["apps_owner"]}
    for name,value in expected.items():
        match=re.search(r"#define "+name+r"\s+(0x[0-9a-f]+|\d+)u?\b",native)
        if not match or int(match[1],0)!=value:raise ValueError("native census ABI differs: "+name)
    if "const uint32_t ids[PROBE_ITEM_COUNT] = {497u};" not in native:
        raise ValueError("native census item differs")
    if "int again = find_items(second, &second_route);" not in native or "if (*lock != HWLOCK_APPS_OWNER)" not in native:
        raise ValueError("native census lacks second scan or release-owner guard")
    tree=ET.parse(ROOT/"platforms/sunfish/ipa-smem-census.system").getroot()
    for name,physical,size in [("smem_snapshot",requirements["smem"]["reserved_physical"],requirements["smem"]["reserved_bytes"]),
                               ("tcsr_smem_lock",requirements["hardware_lock"]["physical"],4096)]:
        mr=tree.find(f"memory_region[@name='{name}']")
        if mr is None or int(mr.get("phys_addr"),0)!=int(physical,0) or int(mr.get("size"),0)!=size:
            raise ValueError("native census physical binding differs: "+name)
    return {"item":497,"requested_bytes":8192,"metadata_scans":2,"smem_read_only":True,
            "abi_constants":expected,"scope":"metadata only; hardware lock claim/release; no SMEM write, allocation, payload reads or startup readiness"}


def audit():
    archive = ROOT / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
    if hashlib.sha256(archive.read_bytes()).hexdigest() != ARCHIVE_SHA256:
        raise ValueError("pinned working kernel archive mismatch")
    paths = ["drivers/interconnect/qcom/sm7150.c", "drivers/net/ipa/data/ipa_data-v4.2.c",
             "drivers/interconnect/qcom/icc-rpmh.c", "drivers/interconnect/qcom/bcm-voter.c",
             "drivers/clk/qcom/clk-rpmh.c", "arch/arm64/boot/dts/qcom/sm7150.dtsi",
             "arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts",
             "drivers/net/ipa/gsi_reg.c", "drivers/net/ipa/reg/gsi_reg-v4.0.c",
             "drivers/net/ipa/reg/ipa_reg-v4.2.c", "drivers/net/ipa/ipa_main.c",
             "drivers/net/ipa/ipa_endpoint.h", "drivers/iommu/arm/arm-smmu/arm-smmu.c",
             "drivers/iommu/arm/arm-smmu/arm-smmu-qcom.c",
             "drivers/net/ipa/ipa_mem.c", "drivers/net/ipa/ipa_mem.h",
             "drivers/net/ipa/gsi.c", "drivers/net/ipa/gsi_private.h", "drivers/net/ipa/gsi_trans.c",
             "drivers/net/ipa/ipa_cmd.c", "drivers/net/ipa/ipa_table.c", "drivers/net/ipa/ipa_endpoint.c",
             "drivers/net/ipa/ipa_resource.c", "drivers/net/ipa/ipa_interrupt.c",
             "drivers/net/ipa/ipa_uc.c", "drivers/net/ipa/ipa_reg.h", "drivers/net/ipa/ipa_version.h",
             "drivers/soc/qcom/smem.c", "drivers/hwspinlock/qcom_hwspinlock.c"]
    with tarfile.open(archive) as t:
        source = {p: t.extractfile("linux-" + PIN + "/" + p).read().decode() for p in paths}
    graph = source[paths[0]]
    nodes = {}
    for name, body in re.findall(r"static struct qcom_icc_node (\w+) = \{(.*?)\n\};", graph, re.S):
        nodes[name] = {"channels": int(re.search(r"\.channels\s*=\s*(\d+)", body)[1]),
                       "width": int(re.search(r"\.buswidth\s*=\s*(\d+)", body)[1]),
                       "links": re.findall(r"&(\w+)", body)}
    bcms = {}
    for _, body in re.findall(r"static struct qcom_icc_bcm (\w+) = \{(.*?)\n\};", graph, re.S):
        name = re.search(r'\.name\s*=\s*"(\w+)"', body)[1]
        bcms[name] = re.findall(r"&(\w+)", body)
    routes = {}
    for name, start, end in [("memory", "qxm_ipa", "ebi"), ("imem", "qxm_ipa", "qxs_imem"),
                              ("config", "acm_apps", "qhs_ipa")]:
        queue = deque([(start, [start])]); seen = set()
        while queue:
            at, path = queue.popleft()
            if at == end:
                routes[name] = path; break
            if at in seen: continue
            seen.add(at)
            for nxt in nodes[at]["links"]:
                queue.append((nxt, path + [nxt]))
        if name not in routes: raise ValueError("missing ICC route: " + name)
    expected = {
        "SN11": (80000, 465000, 16, 16), "SN0": (80000, 465000, 16, 16),
        "SH0": (80000, 465000, 32, 16), "MC0": (80000, 465000, 8, 4),
        "SN1": (0, 68570, 8, 8), "SH5": (0, 30000, 16, 16),
        "SH2": (0, 30000, 8, 8), "SN15": (0, 30000, 8, 8), "CN0": (0, 30000, 4, 4),
    }
    # Independently derive route traffic at every graph node, then compare
    # the narrowest selected BCM members with the native collapsed profile.
    traffic = {"memory": (80000, 465000), "imem": (0, 68570), "config": (0, 30000)}
    contributions = {}
    for route, path in routes.items():
        avg, peak = traffic[route]
        for name in path:
            a, p = contributions.get(name, (0, 0))
            contributions[name] = a + avg, max(p, peak)
    found = {}
    selected = set()
    for bcm, members in bcms.items():
        use = [n for n in members if n in contributions]
        if not use: continue
        selected.add(bcm)
        if bcm == "ACV": continue
        narrow_avg = min(use, key=lambda n: nodes[n]["channels"] * nodes[n]["width"])
        narrow_peak = min(use, key=lambda n: nodes[n]["width"])
        # For this particular map, all selected members of one BCM receive
        # the same traffic. Refuse an unnoticed topology change.
        if len({contributions[n] for n in use}) != 1: raise ValueError("mixed BCM traffic")
        avg, peak = contributions[use[0]]
        found[bcm] = (avg, peak, nodes[narrow_avg]["channels"] * nodes[narrow_avg]["width"], nodes[narrow_peak]["width"])
    if found != expected or selected != set(expected) | {"ACV"}: raise ValueError("native ICC profile mismatch")
    pdata = source[paths[1]]
    for name, (avg, peak) in traffic.items():
        match = re.search(r'\.name\s*=\s*"' + name + r'"(.*?)(?:\n\s*\},)', pdata, re.S)[1]
        if int(re.search(r"\.average_bandwidth\s*=\s*(\d+)", match)[1]) != avg or int(re.search(r"\.peak_bandwidth\s*=\s*(\d+)", match)[1]) != peak:
            raise ValueError("IPA bandwidth mismatch")
    checks = [
        (".core_clock_rate\t= 100 * 1000 * 1000", pdata),
        ("bcm->vote_scale = 1000", source[paths[2]]),
        (".enable_mask = BIT(3)", graph),
        ("le32_to_cpu(data->unit) * 1000ULL", source[paths[4]]),
        ('DEFINE_CLK_RPMH_BCM(ipa, "IP0")', source[paths[4]]),
        ("0x01e04000 0x0 0x2c000", source[paths[5]]),
        ("0x01e40000 0x0 0x7000", source[paths[5]]),
        ('firmware-name = "qcom/sm7150/google/sunfish/ipa_fws.mbn"', source[paths[6]]),
        ("case IPA_VERSION_4_2:\n\t\treturn &gsi_regs_v4_0", source[paths[7]]),
        ("gsi_status, 0x0001f000", source[paths[8]]),
        ("hw_param_2, 0x0001f040", source[paths[8]]),
        ("flavor_0, 0x00000210", source[paths[9]]),
        ("shared_mem_size, 0x00000054", source[paths[9]]),
    ]
    for wanted, text in checks:
        if wanted not in text: raise ValueError("source definition changed: " + wanted)
    ipa_node = re.search(r"ipa: ipa@1e40000 \{(.*?)\n\t\t\};", source[paths[5]], re.S)[1]
    sids = [(int(s,16),int(mask,16)) for s,mask in re.findall(r"<&apps_smmu (0x[0-9a-f]+) (0x[0-9a-f]+)>", ipa_node)]
    if sids != [(0x520,0),(0x522,0)]: raise ValueError("Linux IPA two-SID domain changed")
    native=(ROOT / "rust/src/sunfish_smmu.rs").read_text()
    primary=re.search(r"pub fn plan_ipa_ap_dma_route\(.*?\n\}",native,re.S)[0]
    peer=re.search(r"pub fn plan_ipa_linux_peer_dma_route\(.*?\n\}",native,re.S)[0]
    if not re.search(r"SUNFISH_IPA_AP_DMA_SID,\s*0,\s*\[SUNFISH_IPA_AP_DMA_SID,\s*0\],\s*1",primary):
        raise ValueError("native primary IPA match is not exact")
    if not re.search(r"0x522,\s*0,\s*\[0x522,\s*0\],\s*1",peer):
        raise ValueError("native peer IPA match is not exact")
    entry=(ROOT / "rust/src/pixel_smmu_install.rs").read_text()
    if "plan_ipa_linux_exact_routes_from_snapshot" not in entry or "with_ipa_linux_peer_route" not in entry or "plan_ipa_linux_dma_route_from_snapshot" in entry:
        raise ValueError("native entry must use two exact IPA routes")
    smmu=source["drivers/iommu/arm/arm-smmu/arm-smmu.c"]
    for text in ["smrs[idx].id = sid;", "smrs[idx].mask = mask;", "arm_smmu_master_alloc_smes(dev)",
                 "arm_smmu_master_install_s2crs(cfg, S2CR_TYPE_TRANS,", "smmu_domain->cfg.cbndx, fwspec)"]:
        if text not in smmu: raise ValueError("Linux per-ID allocation/attachment changed: " + text)
    write_sme=re.search(r"static void arm_smmu_write_sme\(.*?\n\}",smmu,re.S)[0]
    if write_sme.index("arm_smmu_write_s2cr") > write_sme.index("arm_smmu_write_smr"):
        raise ValueError("Linux route-register write order changed")
    return {"hardware_configuration": hardware_profile(source), "hardware_source_mutations_rejected": hardware_mutations(source),
            "local_memory_canaries": memory_profile(source), "memory_source_mutations_rejected": memory_mutations(source),
            "buffer_requirements": buffer_profile(source), "buffer_source_mutations_rejected": buffer_mutations(source),
            "resource_configuration": resource_profile(source), "resource_source_mutations_rejected": resource_mutations(source),
            "uc_irq_arm": irq_profile(source), "irq_source_mutations_rejected": irq_mutations(source),
            "shared_memory_prerequisites": shared_memory_profile(source), "shared_memory_source_mutations_rejected": shared_memory_mutations(source),
            "smem_census": census_source_profile(source),
            "kernel_commit": PIN, "archive_sha256": ARCHIVE_SHA256,
            "source_sha256": {p: hashlib.sha256(s.encode()).hexdigest() for p, s in source.items()},
            "routes": routes, "icc_profile": found, "acv_enable_mask": 8,
            "core_clock_hz": 100000000, "clock_unit_multiplier": 1000,
            "ipa_base": "0x01e40000", "gsi_base": "0x01e04000", "ipa_iommu_sids": [hex(s) for s,_ in sids], "native_ipa_route_entries": 2, "native_ipa_route_masks": [0, 0],
            "scope": "Power floors, primitive configuration and local-memory canary subset; no PAS, DMA, modem or WLAN readiness"}


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__); p.add_argument("--output", type=Path, required=True)
    args = p.parse_args(); result = audit(); args.output.write_text(json.dumps(result, indent=2) + "\n")
    print("Pinned Linux IPA power, primitive writes, SRAM, buffers, resource limits and UC IRQ arming verified.")
