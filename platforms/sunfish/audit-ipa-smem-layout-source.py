#!/usr/bin/env python3
"""Keep the stopped guard intact while collecting read-only row evidence."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import re
import tarfile

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("ipa_linux_source", HERE / "audit-native-ipa-linux-source.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
STOPPED = base.ROOT / ".cache/sunfish-native-wifi/private/device-runs/ipa-smem-allocate-20261026T045126Z"


def check(native):
    for token in ["if (!row->offset && !row->size) continue;",
                  "if (!row->offset || !row->size) { cause = 1u; ++result.partial_empty; }",
                  "!range_ok(row->offset, row->size, SMEM_BYTES - SMEM_PTABLE_BYTES)",
                  "cause = 2u; ++result.invalid_range;",
                  "row->offset < route->offset + route->size && route->offset < row->offset + row->size",
                  "cause = 4u; ++result.overlaps;",
                  "if (i == route->partition_index) continue;",
                  "layout_lock_read() != HWLOCK_APPS_OWNER", "if (second || !layout_matches(&layout_first, &layout_second)) result = -30;"]:
        if token not in native:
            raise ValueError("read-only classification/scan policy changed: " + token)
    if re.search(r"\b(?:allocate_once|alloc_write)\s*\(", native):
        raise ValueError("allocation call in read-only variant")


def audit():
    result = base.audit()
    manifest = json.loads((STOPPED / "manifest-before-boot.json").read_text())
    key = "platforms/sunfish/ipa-smem-allocate.inc"
    expected = manifest["source_sha256"][key]
    for path in [STOPPED / "source" / key]:
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError("frozen stopped allocation guard was changed")
    guard = (STOPPED / "source" / key).read_text()
    for token in ["if (!offset && !size) continue;", "if (!offset || !size || !range_ok(offset, size, table)) return -43;",
                  "offset < ALLOC_PART_OFFSET + ALLOC_PART_BYTES && ALLOC_PART_OFFSET < offset + size"]:
        if token not in guard:
            raise ValueError("frozen stopped guard does not match diagnostic policy")
    archive = base.ROOT / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
    with tarfile.open(archive) as handle:
        linux = handle.extractfile("linux-" + base.PIN + "/drivers/soc/qcom/smem.c").read().decode()
    for name in ["qcom_smem_set_global_partition", "qcom_smem_enumerate_partitions"]:
        body = re.search(r"(?:static int\s+)" + name + r"\(.*?\n\}", linux, re.S)[0]
        for token in ["if (!le32_to_cpu(entry->offset))\n\t\t\tcontinue;", "if (!le32_to_cpu(entry->size))\n\t\t\tcontinue;"]:
            if token not in body:
                raise ValueError("re-audit Linux's zero-field row policy")
    native = (HERE / "ipa-smem-layout.inc").read_text()
    check(native)
    cases = [("cause = 1u; ++result.partial_empty;", "cause = 0u; ++result.partial_empty;"),
             ("cause = 2u; ++result.invalid_range;", "cause = 1u; ++result.invalid_range;"),
             ("cause = 4u; ++result.overlaps;", "cause = 0u; ++result.overlaps;"),
             ("if (i == route->partition_index) continue;", "if (i != route->partition_index) continue;"),
             ("layout_lock_read() != HWLOCK_APPS_OWNER", "layout_lock_read() == HWLOCK_APPS_OWNER")]
    for old, new in cases:
        if native.count(old) != 1:
            raise ValueError("layout mutation did not uniquely apply")
        try:
            check(native.replace(old, new))
        except ValueError:
            pass
        else:
            raise ValueError("layout source mutation accepted: " + old)
    result.update({"smem_layout_observer": {"stopped_run": str(STOPPED), "stopped_allocator_source_sha256": expected,
        "metadata_scans": 2, "foreign_row_causes": {"1": "one zero field", "2": "nonzero range invalid", "4": "nonzero in-range overlap"},
        "linux_skips_either_zero_field": True, "guard_relaxed": False, "allocation": False,
        "scope": "read-only explanation of stopped guard; no hypothesis promoted to root cause"},
        "layout_source_mutations_rejected": len(cases)})
    return result


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    args.output.write_text(json.dumps(audit(), indent=2) + "\n")
    print("Frozen stopped guard, pinned zero-field policy and 5 layout-source mutations verified.")
