#!/usr/bin/env python3
"""Bind the native UC dispatcher to the pinned reference interrupt ABI/order."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import tarfile

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("shared_uc_reference", HERE / "audit-native-ipa-shared-source.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
ROOT = base.ROOT
PIN = "2267e248204e7e051e0f7355298949a6440de579"


def audit():
    result = base.audit()  # Includes the full pinned archive hash check.
    names = ["ipa_interrupt.c", "ipa_uc.c", "reg/ipa_reg-v4.2.c"]
    with tarfile.open(ROOT / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz") as tar:
        source = {n: tar.extractfile(f"linux-{PIN}/drivers/net/ipa/{n}").read().decode() for n in names}
    interrupt, uc, regs = (source[n] for n in names)
    process = interrupt.split("static void ipa_interrupt_process(", 1)[1].split("/* IPA IRQ handler", 1)[0]
    if process.index("iowrite32(mask,") > process.index("ipa_uc_interrupt_handler(ipa, irq_id);"):
        raise ValueError("reference no longer clears UC before shared-byte dispatch")
    for token in ["while ((mask = pending & enabled))", "irq_id = __ffs(mask)",
                  "if (pending)", "iowrite32(pending,"]:
        if token not in interrupt:
            raise ValueError("reference interrupt drain/disabled-clear changed: " + token)
    if not re.search(r"REG\(IPA_IRQ_CLR,\s*ipa_irq_clr,\s*0x00003010", regs):
        raise ValueError("reference IPA v4.2 W1C register changed")
    structure = uc.split("struct ipa_uc_mem_area {", 1)[1].split("};", 1)[0]
    structure = re.sub(r"/\*.*?\*/", "", structure, flags=re.S)
    offset, fields = 0, {}
    for declaration in structure.split(";"):
        if not declaration.strip():
            continue
        m = re.fullmatch(r"\s*(u8|__le16|__le32)\s+(\w+)(?:\[(\d+)\])?\s*", declaration)
        if not m:
            raise ValueError("unrecognized UC structure declaration")
        width = {"u8": 1, "__le16": 2, "__le32": 4}[m[1]]
        offset = (offset + width - 1) // width * width
        fields[m[2]] = offset
        offset += width * int(m[3] or 1)
    if fields["response"] != 12 or fields["event"] != 20:
        raise ValueError("UC byte fields changed")
    for token in ["IPA_UC_RESPONSE_INIT_COMPLETED\t= 0x1", "IPA_UC_EVENT_ERROR\t\t= 0x1",
                  "IPA_UC_EVENT_LOG_INFO\t\t= 0x2", "if (ipa->uc_powered)",
                  "ipa->uc_loaded = true;", "ipa_power_retention(ipa, true);"]:
        if token not in uc:
            raise ValueError("UC response/event/power semantics changed: " + token)
    paths = ["rust/src/sunfish_ipa_linux_irq.rs", "rust/src/sunfish_ipa_linux_uc.rs",
             "rust/src/sunfish_native_startup_preflight.rs", "tests/sunfish_ipa_linux_uc_test.rs"]
    native = (ROOT / paths[1]).read_text()
    if not native.index("self.clear(io, mask)?;") < native.index("let value = io.shared(field)?;") < native.index("io.acknowledge()?;"):
        raise ValueError("native UC clear/shared/physical-ACK ordering changed")
    if re.search(r"pub\s+(?:const\s+)?fn\s+\w+[^\n]*(?:Ready|Permit|Lease)", native):
        raise ValueError("dispatcher must not mint production readiness")
    result["uc_dispatch"] = {
        "reference_pin": PIN,
        "reference_sha256": {n: hashlib.sha256(source[n].encode()).hexdigest() for n in names},
        "native_sha256": {p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest() for p in paths},
        "clear_offset": "0x3010", "shared_byte_offsets": {k: fields[k] for k in ["event", "response"]},
        "maximum_enabled_drain_rounds": 64,
        "power": "existing cold-test floors remain held; no PM put or collapse/retention transition",
        "intentional_differences": ["bounded drain", "microcontroller ERROR quarantines startup instead of only logging",
                                    "all I/O/guard/ACK failures mask once and retain resources"],
        "scope": "opt-in pre-PAS runtime integration; GSI-disabled guard; no firmware/UC commands or readiness token",
        "device_test": "not run; no device needed before firmware-start integration",
    }
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(json.dumps(audit(), indent=2) + "\n")
    print("UC clear/read/ACK order and byte ABI bound to the pinned working Linux reference.")
