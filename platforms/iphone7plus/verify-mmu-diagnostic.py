#!/usr/bin/env python3
"""Reject missing exception instrumentation or stale packaged loader code."""
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile


def require(condition, message):
    if not condition:
        raise SystemExit("MMU diagnostic verification failed: " + message)


def main():
    require(len(sys.argv) == 3, "usage: verify-mmu-diagnostic.py LOADER_ELF RAW_PAYLOAD")
    elf, payload = map(Path, sys.argv[1:])
    llvm = Path(os.environ.get("LLVM_BIN", "/opt/homebrew/opt/llvm/bin"))
    symbols = {}
    for line in subprocess.check_output([str(llvm / "llvm-nm"), str(elf)], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            symbols[fields[2]] = int(fields[0], 16)
    for name in ("_start", "_text_end", "arm_vector_table", "pongo_mmu_fault_vector"):
        require(name in symbols, name + " is absent from the compiled ELF")

    raw = payload.read_bytes()
    base = symbols["_start"]
    code_size = symbols["_text_end"] - base
    with tempfile.TemporaryDirectory() as tmp:
        flat = Path(tmp) / "loader.bin"
        subprocess.run([str(llvm / "llvm-objcopy"), "-O", "binary", str(elf), str(flat)], check=True)
        require(raw[:code_size] == flat.read_bytes()[:code_size],
                "packaged code differs from the inspected ELF")

    vector = symbols["arm_vector_table"]
    handler = symbols["pongo_mmu_fault_vector"]
    for index in range(16):
        address = vector + index * 128
        word, = struct.unpack_from("<I", raw, address - base)
        require(word >> 26 == 0b000101, f"vector {index} does not start with an unconditional branch")
        offset = word & 0x3ffffff
        if offset & (1 << 25):
            offset -= 1 << 26
        require(address + offset * 4 == handler, f"vector {index} does not target the stackless handler")

    disassembly = subprocess.check_output([
        str(llvm / "llvm-objdump"), "-d", "--no-show-raw-insn",
        "--disassemble-symbols=pongo_mmu_fault_vector", str(elf)], text=True)
    instructions = []
    for line in disassembly.splitlines():
        match = re.match(r"\s*[0-9a-f]+:\s+(.*)", line)
        if match:
            instructions.append(match[1].split("//")[0].strip())
    require(instructions, "handler has no instructions")
    require(not any(re.search(r"\bsp\b|^(bl|blr|ret)\b", line) for line in instructions),
            "fault handler depends on stack, subroutine calls, or a return address")
    control_write = next((i for i, line in enumerate(instructions) if re.match(r"msr\s+SCTLR_EL1,", line)), None)
    first_store = next((i for i, line in enumerate(instructions) if line.startswith("str\t") or line.startswith("str ")), None)
    require(control_write is not None and first_store is not None and control_write < first_store,
            "framebuffer store precedes translation disable")
    require(any(line == "isb" for line in instructions[control_write:first_store]),
            "missing instruction barrier before framebuffer output")
    # MOVZ W10,#0 is format-independent black. X11/X12 each count 128
    # rows/pixels, avoiding reliance on the uncalibrated color layout.
    handler_bytes = raw[handler-base:handler-base+128]
    require(struct.pack("<I", 0x5280000a) in handler_bytes,
            "fault marker does not encode zero/black")
    require(any(re.match(r"mov\s+x11, #0x80\b", line) for line in instructions),
            "fault marker is not 128 rows high")
    require(any(re.match(r"mov\s+x12, #0x80\b", line) for line in instructions),
            "fault marker is not 128 pixels wide")
    literal_loads = []
    for offset in range(0, 128, 4):
        word, = struct.unpack_from("<I", handler_bytes, offset)
        if word & 0xff00001f == 0x58000009:  # LDR X9, PC-relative literal
            relative = (word >> 5) & 0x7ffff
            if relative & (1 << 18):
                relative -= 1 << 19
            literal, = struct.unpack_from("<Q", raw, handler-base+offset+relative*4)
            literal_loads.append(literal)
    require(0x8BB80C000 in literal_loads, "fault square does not start at framebuffer row zero")
    print("Verified packaged loader: 16 stackless vectors; black square after SCTLR/ISB; code matches ELF.")


if __name__ == "__main__":
    main()
