#!/usr/bin/env python3
"""Bind the allocator correction to IMG_9132 and pinned SMEM enumeration."""
from pathlib import Path
import argparse
import copy
import hashlib
import importlib.util
import json
import re
import tarfile

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("allocation_source_audit", HERE / "audit-ipa-smem-allocate-source.py")
previous = importlib.util.module_from_spec(spec)
spec.loader.exec_module(previous)
ROOT = previous.base.ROOT
LAYOUT_RUN = ROOT / ".cache/sunfish-native-wifi/private/device-runs/ipa-smem-layout-20261026T050609Z"


def check(native, linux, evidence, requirements):
    values = {k: int(v, 16) for k, v in evidence["values"].items()}
    table = requirements["smem"]["reserved_bytes"] - 4096
    expected = {"SCANS_DONE": 2, "RELEASE_SENT": 1, "LOCAL_ERROR": 0, "GUARD_MASK": 3,
                "FIRST_BAD_ROW": 0, "BAD_CAUSE": 2, "BAD_OFFSET": table, "BAD_BYTES": 4096,
                "BAD_HOST_0": 65535, "BAD_HOST_1": 65535, "PARTIAL_EMPTY": 1, "RANGE_FAILS": 1,
                "OVERLAPS": 0, "TARGET_INDEX": 7, "TARGET_OFFSET": 0x11d000, "TARGET_BYTES": 0x7c000,
                "FREE_UNCACHED": 32, "ITEM_PRESENT": 0}
    if evidence["headline"] != "SMEM TABLE MATCH" or any(values.get(k) != v for k, v in expected.items()):
        raise ValueError("layout evidence differs from measured TOC descriptor / absent-item state")
    for name, value in {"ALLOC_EXPECTED_TABLE_COUNT": values["TABLE_COUNT"], "ALLOC_TOC_ROW": values["FIRST_BAD_ROW"],
                        "ALLOC_TOC_HOST": values["BAD_HOST_0"]}.items():
        match = re.search(r"#define\s+" + name + r"\s+(0x[0-9a-f]+|\d+)u?\b", native)
        if not match or int(match[1], 0) != value:
            raise ValueError("correction constant differs from captured row: " + name)
    toc_mapping = re.search(r"static int qcom_smem_map_toc\(.*?\n\}", linux, re.S)[0]
    for token in ["ptable_start = region->aux_base + region->size - SZ_4K;",
                  "smem->ptable = devm_ioremap_wc(smem->dev, ptable_start, SZ_4K);"]:
        if token not in toc_mapping:
            raise ValueError("source TOC location/size changed")
    for name in ["qcom_smem_set_global_partition", "qcom_smem_enumerate_partitions"]:
        body = re.search(r"static int\s+" + name + r"\(.*?\n\}", linux, re.S)[0]
        for token in ["if (!le32_to_cpu(entry->offset))\n\t\t\tcontinue;", "if (!le32_to_cpu(entry->size))\n\t\t\tcontinue;"]:
            if token not in body:
                raise ValueError("source empty-slot policy changed")
    for token in ["#define SMEM_GLOBAL_HOST\t0xfffe", "#define SMEM_HOST_APPS\t\t0",
                  "if (host0 == local_host)", "else if (host1 == local_host)"]:
        if token not in linux:
            raise ValueError("re-audit source partition host selection")
    # No general widening to the SMEM end. Only the exact measured metadata
    # descriptor is excluded; every other nonempty span remains bounded by TOC.
    for token in ["if (count != ALLOC_EXPECTED_TABLE_COUNT) return -47;",
                  "rd32(base, toc) != table", "rd32(base, toc + 4u) != SMEM_PTABLE_BYTES",
                  "rd16(base, toc + 12u) != ALLOC_TOC_HOST", "rd16(base, toc + 14u) != ALLOC_TOC_HOST",
                  "!range_ok(route->offset, route->size, table)",
                  "if (i == route->partition_index || i == ALLOC_TOC_ROW) continue;",
                  "if (!offset || !size) continue;", "if (!range_ok(offset, size, table)) return -43;",
                  "if (offset < route->offset + route->size && route->offset < offset + size) return -43;"]:
        if token not in native:
            raise ValueError("corrected guard lost a required condition: " + token)


def audit():
    result = previous.audit()  # Retain all eight publication/lifecycle mutations.
    archive = ROOT / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
    with tarfile.open(archive) as handle:
        linux = handle.extractfile("linux-" + previous.base.PIN + "/drivers/soc/qcom/smem.c").read().decode()
    path = LAYOUT_RUN / "screen/transcription.json"
    evidence = json.loads(path.read_text())
    for name, expected in evidence["files"].items():
        photo = path.parent / name
        if photo.stat().st_size != expected["bytes"] or hashlib.sha256(photo.read_bytes()).hexdigest() != expected["sha256"]:
            raise ValueError("layout photo archive mismatch")
    native = (HERE / "ipa-smem-allocate-v2-guard.inc").read_text()
    check(native, linux, evidence, result["shared_memory_prerequisites"])
    wrapper = (HERE / "ipa-smem-allocate-v2.c").read_text()
    if "#define SMEM_IPA_ALLOC_V2 1" not in wrapper:
        raise ValueError("v2 wrapper did not select the corrected guard")
    common = (HERE / "ipa-smem-allocate.inc").read_text()
    if '#include "ipa-smem-allocate-v2-guard.inc"' not in common or "#ifdef SMEM_IPA_ALLOC_V2\n    return alloc_foreign_v2(route, count);\n#else" not in common:
        raise ValueError("common allocator did not dispatch the selected guard")
    cases = [("#define ALLOC_EXPECTED_TABLE_COUNT 11u", "#define ALLOC_EXPECTED_TABLE_COUNT 12u"),
             ("#define ALLOC_TOC_ROW 0u", "#define ALLOC_TOC_ROW 1u"),
             ("#define ALLOC_TOC_HOST 0xffffu", "#define ALLOC_TOC_HOST 0xfffeu"),
             ("rd32(base, toc + 4u) != SMEM_PTABLE_BYTES", "rd32(base, toc + 4u) != 0u"),
             ("rd16(base, toc + 14u) != ALLOC_TOC_HOST", "rd16(base, toc + 14u) == ALLOC_TOC_HOST"),
             ("if (!offset || !size) continue;", "if (!offset && !size) continue;"),
             ("!range_ok(offset, size, table)", "!range_ok(offset, size, SMEM_BYTES)"),
             ("route->offset < offset + size", "route->offset > offset + size")]
    for old, new in cases:
        if native.count(old) != 1:
            raise ValueError("TOC guard mutation did not uniquely apply")
        try:
            check(native.replace(old, new), linux, evidence, result["shared_memory_prerequisites"])
        except ValueError:
            pass
        else:
            raise ValueError("TOC guard mutation accepted: " + old)
    changed = copy.deepcopy(evidence)
    changed["values"]["OVERLAPS"] = "00000001"
    try:
        check(native, linux, changed, result["shared_memory_prerequisites"])
    except ValueError:
        pass
    else:
        raise ValueError("overlapping-layout evidence mutation accepted")
    result.update({"corrected_smem_foreign_guard": {"table_count": 11, "toc_row": 0, "toc_offset": "0x1ff000",
        "toc_bytes": 4096, "toc_hosts": [65535, 65535], "linux_either_zero_field_skipped": True,
        "other_nonzero_range_limit": "0x1ff000", "overlap_guard_retained": True,
        "new_error": "47 decimal: changed TOC descriptor/count; no write attempted"},
        "corrected_guard_mutations_rejected": len(cases) + 1,
        "layout_evidence": {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "files": evidence["files"]}})
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(json.dumps(audit(), indent=2) + "\n")
    print("Pinned allocation/publication, observed TOC descriptor, Linux empty-slot policy and 9 corrected-guard mutations verified.")
