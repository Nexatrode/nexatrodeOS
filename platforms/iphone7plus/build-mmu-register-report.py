#!/usr/bin/env python3
"""Insert a pre-enable numeric report without moving any loader/table addresses.

Only the verified baseline is supported: one branch replaces its SCTLR enable
write and a stackless report occupies otherwise unused alignment padding.
This is a terminal diagnostic, not a bootable OS image.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import shutil
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("elf", type=Path)
    parser.add_argument("payload", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--test-16k", action="store_true",
                        help="replace the terminal snapshot with a 16 KiB translation test")
    args = parser.parse_args()
    llvm = Path(os.environ.get("LLVM_BIN", "/opt/homebrew/opt/llvm/bin"))
    here = Path(__file__).resolve().parent
    subprocess.run([sys.executable, str(here / "verify-mmu-diagnostic.py"),
                    str(args.elf), str(args.payload)], check=True)
    raw = args.payload.read_bytes()
    source_hash = hashlib.sha256(raw).hexdigest()
    if source_hash != "8ecb679527ae6d86ecb7d8d663faf16d65eb506108408d0975ff29af6d5f5d02":
        raise SystemExit("This address-preserving report requires the audited 8ecb6795 baseline")
    base, checkpoint, slot, end = 0x810000000, 0x81000129c, 0x810001400, 0x810002000
    if raw[slot-base:end-base] != struct.pack("<I", 0xd503201f) * ((end-slot)//4):
        raise SystemExit("Report slot is not entirely NOP alignment padding")
    if struct.unpack_from("<II", raw, checkpoint-base) != (0xd5181008, 0xd5033fdf):
        raise SystemExit("Checkpoint is not the verified SCTLR write plus ISB")
    args.output.mkdir(parents=True, exist_ok=True)
    obj, elf, flat = [args.output / name for name in ("report.o", "report.elf", "report.bin")]
    subprocess.run([str(llvm / "clang"), "--target=aarch64-none-elf", "-c",
                    "-DEXPECTED_L1=0x810009000", *(["-DMMU_16K_TEST"] if args.test_16k else []),
                    str(here / "loader_mmu_register_report.S"),
                    "-o", str(obj)], check=True)
    linker = shutil.which("ld.lld")
    if linker is None:
        raise SystemExit("ld.lld is required")
    subprocess.run([linker, "-Ttext=" + hex(slot), "-e", "register_report",
                    str(obj), "-o", str(elf)], check=True)
    subprocess.run([str(llvm / "llvm-objcopy"), "--only-section=.text", "-O", "binary",
                    str(elf), str(flat)], check=True)
    code = flat.read_bytes()
    if len(code) > end-slot:
        raise SystemExit("Report exceeds the verified padding slot")
    symbols = {}
    for line in subprocess.check_output([str(llvm / "llvm-nm"), str(elf)], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            symbols[fields[2]] = int(fields[0], 16)
    image = bytearray(raw)
    branch = 0x14000000 | ((slot-checkpoint)//4)
    struct.pack_into("<I", image, checkpoint-base, branch)
    image[slot-base:slot-base+len(code)] = code
    if args.test_16k:
        # All 16 vectors already branch here. Redirect the terminal handler
        # to the numeric fault report; it never returns or advances ELR.
        handler = 0x810002790
        distance = symbols["report_fault"] - handler
        struct.pack_into("<I", image, handler-base, 0x14000000 | ((distance//4) & 0x3ffffff))
    (args.output / "loader.bin").write_bytes(image)
    manifest = {
        "source_sha256": source_hash, "sha256": hashlib.sha256(image).hexdigest(),
        "checkpoint": hex(checkpoint), "branch_instruction": hex(branch),
        "report_start": hex(slot), "report_bytes": len(code),
        "report_wait": hex(symbols["report_wait"]), "report_values": hex(symbols["report_values"]),
        "rows": ["MMUPRE1", "CurrentEL", "SCTLR_EL1", "TCR_EL1", "TTBR0_EL1",
                 "TTBR1_EL1", "MAIR_EL1", "VBAR_EL1", "ID_AA64MMFR0_EL1", "SP",
                 "proposed_SCTLR", "actual_L1_32", "actual_L1_34", "expected_L1_32",
                 "expected_L1_34", "ID_AA64MMFR1_EL1"],
        "expected": "16 indexed hexadecimal rows; MMU enable is deliberately not executed",
    }
    if args.test_16k:
        manifest.update({"mode": "16k-transition", "report_fault": hex(symbols["report_fault"]),
                         "expected": "row 0=0031364B50415353 (16KPASS), row 2 has M=1",
                         "rows": ["status", "CurrentEL", "SCTLR_EL1", "TCR_EL1", "TTBR0_EL1",
                                  "TTBR1_EL1", "MAIR_EL1", "VBAR_EL1", "ID_AA64MMFR0_EL1", "SP",
                                  "proposed_SCTLR", "code_L2", "display_L2", "display_end_L2_or_ESR",
                                  "PAR_code_or_FAR", "PAR_display_or_ELR"]})
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(manifest["sha256"], args.output / "loader.bin")


if __name__ == "__main__":
    main()
