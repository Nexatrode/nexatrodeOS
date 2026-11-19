#!/usr/bin/env python3
"""Prepare a compact entry hold and an optional inert-payload size comparison.

These images draw the same splash marker then loop. They never relocate,
start seL4, inspect firmware, call PAS, or access modem/resource MMIO.
The script builds local artifacts only; it never calls ADB or Fastboot.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import struct
import subprocess

PORT = Path(__file__).resolve().parent
ROOT = PORT.parents[1]
ENTRY = 0x80081000


def load(name):
    spec = importlib.util.spec_from_file_location(name, PORT / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sha(data):
    return hashlib.sha256(data).hexdigest()


def validate_entry(image, entry_offset, hold_offset):
    """Pin the executed instruction path, including the terminal self-branch."""
    if entry_offset != 0 or hold_offset != 24 or len(image) < 32:
        raise ValueError('unexpected entry/hold offsets')
    words = struct.unpack_from('<7I', image)
    if (words[0] & 0x9f00001f != 0x90000001  # ADRP x1, stack
            or words[1] & 0xffc003ff != 0x91000021  # ADD x1,x1,#imm
            or words[2] != 0x9100003f  # MOV sp,x1
            or words[3] != 0xd5034fdf  # MSR DAIFSet,#15
            or words[4] & 0x9f00001f != 0x10000000  # ADR x0, label
            or words[5] & 0xfc000000 != 0x94000000  # BL drawing function
            or words[6] != 0x14000000):  # B . (no fall-through)
        raise ValueError('entry must draw once and terminate in a self-branch')
    displacement = words[5] & 0x03ffffff
    if displacement & (1 << 25):
        displacement -= 1 << 26
    target = 20 + 4 * displacement
    if not (32 <= target < len(image)):
        raise ValueError('drawing call target outside entry image')
    return target


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out-dir', type=Path,
                        default=ROOT / '.cache/pixel-sunfish/entry-hold-check')
    parser.add_argument('--comparison-payload', type=Path)
    parser.add_argument('--comparison-sha256')
    args = parser.parse_args()
    if bool(args.comparison_payload) != bool(args.comparison_sha256):
        parser.error('comparison payload and its pinned SHA-256 are both required')
    out = args.out_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    llvm = Path('/opt/homebrew/opt/llvm/bin')
    env = dict(os.environ, PATH=str(llvm) + ':/opt/homebrew/bin:' + os.environ['PATH'])

    def run(*command):
        subprocess.run([str(c) for c in command], env=env, check=True)

    drawing = out / 'entry-drawing.c'
    drawing.write_text('#include "loader-boot-checkpoints.h"\n')
    linker = out / 'entry.ld'
    linker.write_text('''ENTRY(_start)
PHDRS { image PT_LOAD FLAGS(7); }
SECTIONS {
    . = 0x80081000;
    .text : { *(.text.start) *(.text*) *(.rodata*) } :image
    .data : { *(.data*) *(.entry_stack) } :image
    .bss : { *(.bss*) *(COMMON) } :image
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
''')
    common = ['--target=aarch64-none-elf', '-mcpu=cortex-a55', '-mgeneral-regs-only',
              '-mstrict-align', '-ffreestanding', '-nostdlib', '-O2', '-Wall', '-Wextra', '-Werror']
    run(llvm / 'clang', *common, '-I', PORT, '-c', drawing, '-o', out / 'drawing.o')
    run(llvm / 'clang', *common, '-c', PORT / 'entry-hold-check.S', '-o', out / 'entry.o')
    elf = out / 'entry.elf'
    run('ld.lld', '-T', linker, out / 'entry.o', out / 'drawing.o', '-o', elf)
    undefined = subprocess.check_output([str(llvm / 'llvm-nm'), '-u', str(elf)], env=env)
    assert not undefined.strip(), undefined
    symbols = {}
    for line in subprocess.check_output([str(llvm / 'llvm-nm'), '-n', str(elf)], env=env, text=True).splitlines():
        address, _, name = line.split()
        symbols[name] = int(address, 16)
    # Turn the one PT_LOAD segment into bytes, including its stack allocation.
    segments = load('prepare-image-loader-sdk').segments(elf)
    assert len(segments) == 1
    virtual, physical, memory_bytes, code = segments[0]
    assert virtual == physical == ENTRY and memory_bytes == len(code)
    assert len(code) <= 0x10000 and symbols['entry_stack_top'] == ENTRY + len(code)
    assert code[-4096:] == bytes(4096)
    target = validate_entry(code, symbols['_start'] - ENTRY, symbols['entry_hold'] - ENTRY)
    assert target == symbols['loader_checkpoint'] - ENTRY
    assert b'IMAGE ENTRY HOLD\0' in code
    (out / 'entry.bin').write_bytes(code)
    disassembly = subprocess.check_output([str(llvm / 'llvm-objdump'), '-d', str(elf)], env=env, text=True)
    (out / 'entry.disassembly.txt').write_text(disassembly)
    # No secure calls, exception returns, or loader/kernel entry.
    for mnemonic in ['\tsmc\t', '\thvc\t', '\tsvc\t', '\teret']:
        assert mnemonic not in disassembly
    assert 'relocate' not in symbols and 'start_kernel' not in symbols

    name = b'TRAILER!!!\0'
    fields = [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, len(name), 0]
    cpio = b'070701' + b''.join(('%08x' % n).encode() for n in fields) + name
    cpio += b'\0' * (-len(cpio) % 4)
    ramdisk = gzip.compress(cpio, mtime=0)
    reference = (Path.home() / 'Backups/pixel-sunfish-20261006/live-slots/boot_a.img').read_bytes()
    pack = load('repack-boot-v2')
    wrapper = load('wrap-microkit-image')
    variants = [('compact', b'')]
    if args.comparison_payload:
        payload = args.comparison_payload.read_bytes()
        assert sha(payload) == args.comparison_sha256, 'comparison payload hash mismatch'
        variants.append(('large', payload))
    audit = load('audit-capdl-memory')
    layout = load('validate-image-boot-layout')
    live = ROOT / '.cache/pixel-sunfish/live-runtime.dtb'
    cells = audit.read_cells(live, '/memory', 'reg')
    assert cells and len(cells) % 4 == 0
    ram = [((cells[i] << 32) | cells[i + 1], (cells[i + 2] << 32) | cells[i + 3])
           for i in range(0, len(cells), 4)]
    fixed = [audit.read_regions(p)[0] for p in [audit.DEFAULT_DTB, live]]
    records = []
    for variant, payload in variants:
        folder = out / variant
        folder.mkdir(exist_ok=True)
        image = wrapper.wrap(code + payload)
        for reserved in fixed:
            layout.validate_range('entry diagnostic Image', 0x80080000, len(image), ram, reserved)
        assert 0x80080000 + len(image) <= 0x85700000
        (folder / 'entry.Image').write_bytes(image)
        run('lz4', '-z', '-f', '-q', folder / 'entry.Image', folder / 'entry.Image.lz4')
        compressed = (folder / 'entry.Image.lz4').read_bytes()
        assert subprocess.check_output(['lz4', '-d', '-c', str(folder / 'entry.Image.lz4')], env=env) == image
        boot = pack.pack(reference, compressed, ramdisk)
        (folder / 'fastboot.img').write_bytes(boot)
        _, parts, _ = pack.components(boot)
        _, original, _ = pack.components(reference)
        assert parts['kernel'] == compressed and parts['ramdisk'] == ramdisk and parts['dtb'] == original['dtb']
        records.append({'variant': variant, 'artifact': str((folder / 'fastboot.img').relative_to(ROOT)),
                        'artifact_sha256': sha(boot), 'boot_bytes': len(boot),
                        'image_bytes': len(image), 'image_end': hex(0x80080000 + len(image)),
                        'entry_bytes': len(code), 'entry_sha256': sha(code),
                        'inert_payload_bytes': len(payload), 'inert_payload_sha256': sha(payload),
                        'native_result': None, 'native_screen_observed': False})
    result = {'mode': 'Draw IMAGE ENTRY HOLD, then loop before relocation or kernel entry',
              'expected_screen': 'IMAGE ENTRY HOLD', 'executed_entry_physical': hex(ENTRY),
              'only_device_writes': 'existing splash framebuffer, bounded strip in both buffers',
              'pas_calls': 0, 'resource_votes': 0, 'modem_memory_access': False,
              'firmware_execution': False, 'sel4_started': False, 'partition_flashed': False,
              'comparison_scope': 'Same executed bytes, wrapper, ramdisk and DTB; only inert appended payload and image size differ. No modem staging result is inferred.',
              'variants': records,
              'source_sha256': {str(p.relative_to(ROOT)): sha(p.read_bytes()) for p in [
                  PORT / 'entry-hold-check.S', PORT / 'loader-boot-checkpoints.h',
                  Path(__file__), PORT / 'wrap-microkit-image.py', PORT / 'repack-boot-v2.py']}}
    (out / 'candidate.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(records, indent=2))


if __name__ == '__main__':
    main()
