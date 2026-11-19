#!/usr/bin/env python3
"""Bind the narrow allocator to pinned Linux and the captured cold census."""
from pathlib import Path
import argparse
import copy
import hashlib
import importlib.util
import json
import re
import tarfile

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("linux_ipa_audit", HERE / "audit-native-ipa-linux-source.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
RUN = base.ROOT / ".cache/sunfish-native-wifi/private/device-runs/ipa-smem-census-20261026T043517Z"


def check(native, linux, requirements, evidence):
    def define(text, name):
        match = re.search(r"#define\s+" + name + r"\s+(0x[0-9a-f]+|\d+)u?\b", text)
        if not match:
            raise ValueError("missing scalar definition: " + name)
        return int(match[1], 0)
    if evidence["headline"] != "SMEM METADATA MATCH":
        raise ValueError("census did not match")
    values = {k: int(v, 16) for k, v in evidence["values"].items()}
    if any(values[k] != v for k, v in {"SCANS_DONE": 2, "LOCK_ATTEMPTS": 1, "RELEASE_SENT": 1,
                                      "GLOBAL_ROUTE": 0, "ITEM_PRESENT": 0, "LOCAL_ERROR": 0}.items()):
        raise ValueError("census is not the observed absent-item private route")
    expected = {"ALLOC_PART_INDEX": values["PART_INDEX"], "ALLOC_PART_OFFSET": values["PART_OFFSET"],
                "ALLOC_PART_BYTES": values["PART_BYTES"], "ALLOC_CACHELINE": values["CACHELINE"],
                "ALLOC_FREE_BEFORE": values["FREE_UNCACHED"], "ALLOC_ITEM_ID": requirements["smem"]["item"],
                "ALLOC_ITEM_BYTES": requirements["smem"]["requested_bytes"]}
    for name, value in expected.items():
        if define(native, name) != value:
            raise ValueError("native constant differs from source/census: " + name)
    if values["SMEM_BASE"] != int(requirements["smem"]["reserved_physical"], 0) or values["SMEM_BYTES"] != requirements["smem"]["reserved_bytes"]:
        raise ValueError("census SMEM binding differs from source")
    if values["FREE_CACHED"] != values["PART_BYTES"] or values["FREE_UNCACHED"] != 32:
        raise ValueError("only the measured empty partition is accepted")
    # Independently verify Linux's payload alignment and the writable fields.
    linux_alloc = re.search(r"static int qcom_smem_alloc_private\(.*?\n\}", linux, re.S)[0]
    for token in ["alloc_size = sizeof(*hdr) + ALIGN(size, 8);", "hdr->canary = SMEM_PRIVATE_CANARY;",
                  "hdr->item = cpu_to_le16(item);", "hdr->size = cpu_to_le32(ALIGN(size, 8));",
                  "hdr->padding_data = cpu_to_le16(le32_to_cpu(hdr->size) - size);", "hdr->padding_hdr = 0;",
                  "le32_add_cpu(&phdr->offset_free_uncached, alloc_size);"]:
        if token not in linux_alloc:
            raise ValueError("pinned private allocation ABI changed: " + token)
    if not linux_alloc.index("hdr->padding_hdr = 0;") < linux_alloc.index("wmb();") < linux_alloc.index("le32_add_cpu"):
        raise ValueError("pinned publication order changed")
    common = (HERE / "smem-directory-probe.c").read_text()
    if define(common, "SMEM_CANARY") != define(linux, "SMEM_PRIVATE_CANARY"):
        raise ValueError("allocation canary differs")
    if define(linux, "SMEM_ITEM_COUNT") != 512 or define(linux, "SMEM_ITEM_LAST_FIXED") != 8:
        raise ValueError("source item limits changed")
    info_function = re.search(r"static u32 qcom_smem_get_item_count\(.*?\n\}", linux, re.S)[0]
    for token in ["&ptable->entry[le32_to_cpu(ptable->num_entries)]", "return SMEM_ITEM_COUNT;", "le16_to_cpu(info->num_items)"]:
        if token not in info_function:
            raise ValueError("source optional item-limit metadata changed")
    if "__le16 num_items;" not in linux or "u8 magic[4];\n\t__le32 size;\n\t__le32 base_addr;\n\t__le32 reserved;\n\t__le16 num_items;" not in linux:
        raise ValueError("source SIII num_items offset changed")
    calls = re.findall(r"result = alloc_write\((.*?)\);", native)
    if calls != ["ALLOC_HEADER_OFFSET, (ALLOC_ITEM_ID << 16) | SMEM_CANARY, 1u",
                 "ALLOC_HEADER_OFFSET + 4u, ALLOC_ITEM_BYTES, 2u",
                 "ALLOC_HEADER_OFFSET + 8u, 0u, 4u",
                 "ALLOC_PART_OFFSET + 12u, ALLOC_FREE_AFTER, 8u"]:
        raise ValueError("native metadata write sequence changed")
    for token in ["#define ALLOC_HEADER_OFFSET (ALLOC_PART_OFFSET + ALLOC_FREE_BEFORE)",
                  "#define ALLOC_FREE_AFTER (ALLOC_FREE_BEFORE + SMEM_PRIVATE_ENTRY + ALLOC_ITEM_BYTES)",
                  "if (alloc_consumed) return -40;", "alloc_consumed = 1u;", "if (!alloc_owned()) return -31;",
                  "alloc_submitted |= bit;", "*word = value;", "if (rd32(smem(), offset) != value) return -45;",
                  "snapshot_barrier();\n    result = alloc_write(ALLOC_PART_OFFSET + 12u, ALLOC_FREE_AFTER, 8u);",
                  "if (!alloc_owned()) result = -31;", "alloc_lock_write(0u); snapshot_barrier(); census_release_sent = 1u;",
                  "ALLOC_ITEM_ID >= alloc_item_count", "smem_metadata_vaddr != smem_vaddr + ALLOC_PART_OFFSET"]:
        if token not in native:
            raise ValueError("native allocation safety/ABI binding changed: " + token)
    if not native.index("alloc_submitted |= bit;") < native.index("*word = value;") < native.index("alloc_completed |= bit;"):
        raise ValueError("uncertain write no longer consumed before store")
    header_bytes = requirements["metadata_layouts"]["smem_private_entry"]["bytes"]
    size = expected["ALLOC_ITEM_BYTES"]
    alloc_size = header_bytes + ((size + 7) & ~7)
    item_offset = values["PART_OFFSET"] + values["FREE_UNCACHED"] + header_bytes
    pa = values["SMEM_BASE"] + item_offset
    return {"item": expected["ALLOC_ITEM_ID"], "requested_bytes": size,
            "allocation_bytes": alloc_size, "new_uncached_free": hex(values["FREE_UNCACHED"] + alloc_size),
            "expected_item_physical_if_unchanged": hex(pa), "future_map_base": hex(pa & ~4095),
            "future_map_pages": ((pa & 4095) + size + 4095) // 4096,
            "planned_smem_write_words": 4, "item_payload_access": False,
            "scope": "one narrow experimental allocation/get; no SMMU map, DMA lease or firmware readiness"}


def audit():
    result = base.audit()
    archive = base.ROOT / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
    with tarfile.open(archive) as handle:
        linux = handle.extractfile("linux-" + base.PIN + "/drivers/soc/qcom/smem.c").read().decode()
    evidence_path = RUN / "screen/transcription.json"
    evidence = json.loads(evidence_path.read_text())
    for name, expected in evidence["files"].items():
        path = RUN / "screen" / name
        if path.stat().st_size != expected["bytes"] or hashlib.sha256(path.read_bytes()).hexdigest() != expected["sha256"]:
            raise ValueError("census photo archive mismatch: " + name)
    native = (HERE / "ipa-smem-allocate.inc").read_text()
    requirements = result["shared_memory_prerequisites"]
    profile = check(native, linux, requirements, evidence)
    cases = [("#define ALLOC_ITEM_ID 497u", "#define ALLOC_ITEM_ID 496u"),
             ("#define ALLOC_ITEM_BYTES 0x2000u", "#define ALLOC_ITEM_BYTES 0x1000u"),
             ("#define ALLOC_PART_OFFSET 0x11d000u", "#define ALLOC_PART_OFFSET 0x11e000u"),
             ("snapshot_barrier();\n    result = alloc_write(ALLOC_PART_OFFSET + 12u", "result = alloc_write(ALLOC_PART_OFFSET + 12u"),
             ("if (alloc_consumed) return -40;", "if (alloc_consumed) return 0;"),
             ("if (!alloc_owned()) result = -31;", "if (!alloc_owned()) result = 0;"),
             ("ALLOC_ITEM_ID >= alloc_item_count", "ALLOC_ITEM_ID > alloc_item_count")]
    for old, new in cases:
        if native.count(old) != 1:
            raise ValueError("allocation mutation did not uniquely apply")
        try:
            check(native.replace(old, new), linux, requirements, evidence)
        except ValueError:
            pass
        else:
            raise ValueError("allocation source mutation accepted: " + old)
    changed = copy.deepcopy(evidence)
    changed["values"]["ITEM_PRESENT"] = "00000001"
    try:
        check(native, linux, requirements, changed)
    except ValueError:
        pass
    else:
        raise ValueError("present-item census mutation accepted")
    result.update({"smem_allocation_experiment": profile, "allocation_mutations_rejected": len(cases) + 1,
                   "census_evidence": {"path": str(evidence_path), "sha256": hashlib.sha256(evidence_path.read_bytes()).hexdigest(),
                                       "files": evidence["files"]}})
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(json.dumps(audit(), indent=2) + "\n")
    print("Pinned Linux allocation ABI, publication order, cold census binding and 8 mutations verified.")
