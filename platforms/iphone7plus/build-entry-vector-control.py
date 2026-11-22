#!/usr/bin/env python3
"""Make an exception checkpoint from the verified loader, with a byte audit.

Only three instructions change: main's two entry pixel values become zero,
and one selected instruction becomes SVC. The default is just after vector
installation; other checkpoints bracket the SCTLR write/ISB that enables MMU.
The source artifact is never modified. This image deliberately does not boot.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("elf", type=Path)
    parser.add_argument("payload", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--checkpoint", choices=("entry", "before-mmu", "after-mmu"), default="entry")
    args = parser.parse_args()
    elf, payload, output = args.elf, args.payload, args.output
    llvm = Path(os.environ.get("LLVM_BIN", "/opt/homebrew/opt/llvm/bin"))
    subprocess.run([sys.executable, str(Path(__file__).with_name("verify-mmu-diagnostic.py")),
                    str(elf), str(payload)], check=True)
    symbols = {}
    for line in subprocess.check_output([str(llvm / "llvm-nm"), str(elf)], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            symbols[fields[2]] = int(fields[0], 16)
    assembly = subprocess.check_output([str(llvm / "llvm-objdump"), "-d", "--no-show-raw-insn",
                                       "--disassemble-symbols=main", str(elf)], text=True)
    colors = re.findall(r"^\s*([0-9a-f]+):\s+mov\s+w([23]), #-0x1000000\b", assembly, re.M)
    vector_calls = re.findall(r"^\s*([0-9a-f]+):\s+bl\s+0x[0-9a-f]+ <arch_set_exception_handler>", assembly, re.M)
    if len(colors) != 2 or {reg for _, reg in colors} != {"2", "3"} or len(vector_calls) != 1:
        raise SystemExit("Unexpected main layout; refusing to patch an unrecognized payload")
    raw = payload.read_bytes()
    image = bytearray(raw)
    base = symbols["_start"]
    changes = []

    def replace(address, word, purpose):
        offset = address - base
        before, = struct.unpack_from("<I", image, offset)
        struct.pack_into("<I", image, offset, word)
        changes.append({"address": hex(address), "before": hex(before), "after": hex(word), "purpose": purpose})

    for address, register in colors:
        replace(int(address, 16), 0x2a1f03e0 | int(register), "zero/black entry pixel value")
    fault = int(vector_calls[0], 16) + 4
    word, = struct.unpack_from("<I", image, fault - base)
    immediate = word & 0x3ffffff
    if immediate & (1 << 25):
        immediate -= 1 << 26
    if word >> 26 != 0b100101 or fault + immediate * 4 != symbols["arch_init"]:
        raise SystemExit("Instruction after vector setup is not BL arch_init")
    purpose = "intentional SVC after VBAR installation, before arch_init/MMU"
    if args.checkpoint != "entry":
        mmu_assembly = subprocess.check_output([
            str(llvm / "llvm-objdump"), "-d", "--no-show-raw-insn",
            "--disassemble-symbols=el1_mmu_enable", str(elf)], text=True)
        writes = re.findall(r"^\s*([0-9a-f]+):\s+msr\s+SCTLR_EL1, x8\b", mmu_assembly, re.M)
        if len(writes) != 1:
            raise SystemExit("Expected exactly one SCTLR_EL1 write in el1_mmu_enable")
        fault = int(writes[0], 16)
        if struct.unpack_from("<I", image, fault-base+4)[0] != 0xd5033fdf:
            raise SystemExit("SCTLR write is not immediately followed by ISB")
        if args.checkpoint == "before-mmu":
            purpose = "intentional SVC in place of SCTLR_EL1 enable write, after table/TLB setup"
        else:
            fault += 8
            purpose = "intentional SVC immediately after SCTLR_EL1 enable and ISB"
    replace(fault, 0xd4000001 | (0x51 << 5), purpose)
    output.mkdir(parents=True, exist_ok=True)
    (output / "loader.bin").write_bytes(image)
    manifest = {"source_sha256": hashlib.sha256(raw).hexdigest(),
                "sha256": hashlib.sha256(image).hexdigest(), "changes": changes,
                "checkpoint": args.checkpoint,
                "expected": "128x128 black square at top left if this exception checkpoint is reached and handled"}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(manifest["sha256"], output / "loader.bin")


if __name__ == "__main__":
    main()
