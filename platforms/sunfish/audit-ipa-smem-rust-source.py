#!/usr/bin/env python3
"""Bind the retained Rust SMEM owner to pinned Linux and the allocation proof."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("smem_c_audit", HERE / "audit-ipa-smem-allocate-v2-source.py")
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)
ROOT = reference.ROOT
RUN = ROOT / ".cache/sunfish-native-wifi/private/device-runs/ipa-smem-allocate-v2-20261026T052736Z"


def check(owner, adapter, requirements, values):
    expected = {
        "SMEM_PHYSICAL": int(requirements["smem"]["reserved_physical"], 0),
        "SMEM_BYTES": requirements["smem"]["reserved_bytes"],
        "LOCK_PHYSICAL": int(requirements["hardware_lock"]["physical"], 0),
        "ITEM_ID": requirements["smem"]["item"],
        "ITEM_BYTES": requirements["smem"]["requested_bytes"],
        "PARTITION_INDEX": values["PART_INDEX"],
        "PARTITION_OFFSET": values["PART_OFFSET"],
        "PARTITION_BYTES": 0x7c000,
        "TABLE_COUNT": 11,
    }
    for name, value in expected.items():
        m = re.search(r"(?:pub )?const " + name + r": \w+ = (0x[\da-f_]+|[\d_]+);", owner)
        if m is None or int(m[1].replace("_", ""), 0) != value:
            raise ValueError("Rust/source allocation geometry differs: " + name)
    for token in [
        "io: I,", "pub unsafe trait Io: Read", "directory::allocation_directory(&self.io, ITEM_ID)",
        "self.report.consumed = true;", "for attempt in 1..=LOCK_ATTEMPT_LIMIT",
        "self.read16(TABLE + 44) != 0xffff", "self.read16(TABLE + 46) != 0xffff",
        "if offset == 0 || bytes == 0", "bytes > TABLE - offset",
        "PARTITION_OFFSET < offset + bytes", "self.report.submitted |= 1 << step;",
        "self.report.completed |= 1 << step;", "self.read32(offset) != value",
        "self.io.barrier();\n            }\n            self.store(step)?;",
        "self.report.post_scans = 2;", "self.item = Some(result?);",
        "return Err(Error::LockLost);\n        }\n        self.io.lock_write(0);",
    ]:
        if token not in owner:
            raise ValueError("retained Rust lifecycle changed: " + token)
    if not owner.index("self.report.submitted |=") < owner.index(".store_metadata(offset, value)") < owner.index("self.report.completed |="):
        raise ValueError("uncertain-store consumption/readback order changed")
    for token in [
        "smem_physical != SMEM_PHYSICAL", "metadata_physical != METADATA_PHYSICAL",
        "lock_physical != LOCK_PHYSICAL", "first.checked_add(PARTITION_OFFSET) != Some(metadata as usize)",
        "STORES.get(self.next_store) != Some(&(offset, value))", "self.lock_read() != 1",
        "self.next_store += 1;", "asm!(\"dsb sy\"", ".write_volatile(value);",
    ]:
        if token not in adapter:
            raise ValueError("native SMEM window guard changed: " + token)


def audit():
    result = reference.audit()
    path = RUN / "screen/transcription.json"
    evidence = json.loads(path.read_text())
    values = {name: int(value, 16) for name, value in evidence["values"].items()}
    if evidence["headline"] != "SMEM ALLOC GET PASS" or any(values[k] != v for k, v in {
        "WRITES_SENT": 15, "WRITES_DONE": 15, "PUBLISH_VERIFIED": 1,
        "PRE_SCANS": 2, "POST_SCANS": 2, "RELEASE_SENT": 1, "LOCAL_ERROR": 0,
        "ITEM_PA": 0x8611d030, "ITEM_BYTES": 8192, "FREE_AFTER": 0x2030,
    }.items()):
        raise ValueError("successful allocation proof changed")
    for name, meta in evidence["files"].items():
        p = path.parent / name
        if p.stat().st_size != meta["bytes"] or hashlib.sha256(p.read_bytes()).hexdigest() != meta["sha256"]:
            raise ValueError("allocation proof file differs")
    owner = (ROOT / "rust/src/sunfish_ipa_smem.rs").read_text()
    adapter = (ROOT / "rust/src/sunfish_ipa_smem_native.rs").read_text()
    requirements = result["shared_memory_prerequisites"]
    check(owner, adapter, requirements, values)
    cases = [
        ("owner", "ITEM_ID: u16 = 497;", "ITEM_ID: u16 = 498;"),
        ("owner", "ITEM_BYTES: usize = 0x2000;", "ITEM_BYTES: usize = 0x1000;"),
        ("owner", "PARTITION_OFFSET: usize = 0x11d000;", "PARTITION_OFFSET: usize = 0x11c000;"),
        ("owner", "TABLE_COUNT: usize = 11;", "TABLE_COUNT: usize = 12;"),
        ("owner", "if offset == 0 || bytes == 0", "if offset == 0 && bytes == 0"),
        ("owner", "self.report.submitted |= 1 << step;", "/* submitted only after write */"),
        ("owner", "PARTITION_OFFSET < offset + bytes", "PARTITION_OFFSET > offset + bytes"),
        ("adapter", "metadata_physical != METADATA_PHYSICAL", "metadata_physical == METADATA_PHYSICAL"),
        ("adapter", "self.next_store += 1;", "/* retry permitted */"),
    ]
    for target, old, new in cases:
        text = owner if target == "owner" else adapter
        if text.count(old) != 1:
            raise ValueError("source mutation did not uniquely apply: " + old)
        try:
            check(owner.replace(old, new) if target == "owner" else owner,
                  adapter.replace(old, new) if target == "adapter" else adapter,
                  requirements, values)
        except ValueError:
            continue
        raise ValueError("source mutation accepted: " + old)
    result["rust_retained_smem_owner"] = {
        "mutations_rejected": len(cases), "host": 1, "item": 497,
        "allocation_proof": {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()},
        "scope": "Owned native metadata/lock view, one attempt, four fixed stores, two pre/post scans; no payload, SMMU, DMA or readiness capability. Not yet wired into a device image.",
    }
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.write_text(json.dumps(audit(), indent=2) + "\n")
    print("Rust SMEM owner bound to pinned Linux and allocation proof; 9 mutations rejected.")
