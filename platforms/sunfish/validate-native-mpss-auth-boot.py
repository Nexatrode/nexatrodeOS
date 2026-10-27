#!/usr/bin/env python3
"""Check complete large-image relocation and every boot copy destination."""
from pathlib import Path
import argparse
import importlib.util
import json
import hashlib
import struct
import subprocess
import re

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def module(name):
    s = importlib.util.spec_from_file_location(name, HERE / (name + '.py'))
    result = importlib.util.module_from_spec(s)
    s.loader.exec_module(result)
    return result


def require(value, text):
    if not value:
        raise ValueError(text)


def validate_source_diagnostic(profile, out):
    """Audit diagnostic guard branches in generated assembly and AArch64 ELF."""
    code_path = out / 'loader-checkpoint-source/src/aarch64/crt0.S'
    elf_path = out / 'loader-relocated-build/loader.elf'
    code = code_path.read_text()
    guard = code.split('    /* May not have been loaded in the right location.', 1)[0]
    for branch in ('b.ne .Lnative_source_address_fail',
                   'b.hi .Lnative_source_prefix_fail',
                   'b.cs .Lnative_source_overflow_fail',
                   'b.hi .Lnative_source_extent_fail'):
        require(branch in guard, 'diagnostic source guard branch: ' + branch)
    for title, label, register in (
            ('SOURCE ADDRESS', '.Lnative_source_address_fail:', 'x0'),
            ('SOURCE PREFIX SIZE', '.Lnative_source_prefix_fail:', 'x3'),
            ('SOURCE SIZE OVERFLOW', '.Lnative_source_overflow_fail:', 'x3'),
            ('SOURCE EXTENT', '.Lnative_source_extent_fail:', 'x3')):
        value_move = re.escape(label) + r'\n\s+mov x1, ' + register
        require(re.search(value_move, guard) is not None, 'diagnostic value for ' + title)
        require('.asciz "' + title + '"' in guard, 'diagnostic title for ' + title)
    stop = '.Lnative_source_diagnostic_stop:\n    b .Lnative_source_diagnostic_stop'
    require(stop in guard and guard.count('bl loader_checkpoint_hex') == 6,
            'diagnostic pass and failures all halt before relocation')
    require('IMAGE SOURCE STOP' not in guard, 'diagnostic never falls back to ambiguous stop')
    disassembly = subprocess.check_output(
        ['/opt/homebrew/opt/llvm/bin/llvm-objdump', '-d', str(elf_path)], text=True)
    relocate = re.search(r'^([0-9a-f]+) <relocate>:$', disassembly, re.M)
    require(relocate is not None, 'compiled relocate symbol')
    relocate_address = format(int(relocate.group(1), 16), 'x')
    relocate_call = re.search(
        r'^([0-9a-f]+):\s+\S+\s+bl\s+0x' + relocate_address + r'\s+<relocate>$',
        disassembly, re.M)
    require(relocate_call is not None, 'compiled call to relocate')
    machine_guard = disassembly[:relocate_call.start()]
    require(machine_guard.count('<loader_checkpoint_hex>') == 6,
            'all six source diagnostics compiled to the AArch64 image')
    self_loops = re.findall(r'^([0-9a-f]+):\s+\S+\s+b\s+0x([0-9a-f]+)(?:\s|$)', machine_guard, re.M)
    require(any(address == target for address, target in self_loops),
            'compiled diagnostic has an unconditional stop loop before relocate')
    require(not re.search(r'^([0-9a-f]+):\s+\S+\s+(?:b(?:\.[a-z]+)?|bl)\s+0x'
                              + relocate_address + r'(?:\s|$)', machine_guard, re.M),
            'no source diagnostic branch reaches relocate')
    return {'generated_assembly_sha256': hashlib.sha256(code_path.read_bytes()).hexdigest(),
            'compiled_loader_sha256': hashlib.sha256(elf_path.read_bytes()).hexdigest(),
            'diagnostic_calls': 6, 'all_paths_stop_before_relocation': True}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output-dir', type=Path, required=True)
    p.add_argument('--reference', type=Path)
    p.add_argument('--loader-only', action='store_true',
                   help='Audit the generated source-diagnostic loader without requiring a packed image')
    a = p.parse_args()
    out = a.output_dir
    if a.loader_only:
        profile = json.loads((out / 'loader-sdk-profile.json').read_text())
        require(profile.get('native_source_diagnostic', False), 'source-diagnostic loader profile')
        require(profile['native_startup_entry_guard'] and profile['base_sdk_unchanged']
                and profile['kernel_initialiser_monitor_unchanged']
                and not profile['task_images'] and profile['entry_interrupt_masked'],
                'closed diagnostic loader profile')
        require(profile['loader_link_address'] == '0xc0000000'
                and profile['expected_runtime_source'] == '0xb0081000'
                and profile['native_startup_max_relocation_bytes'] == 0x6000000,
                'exact runtime source/envelope guard')
        result = validate_source_diagnostic(profile, out)
        (out / 'source-guard-loader-audit.json').write_text(json.dumps(result, indent=2) + '\n')
        print('PASS: generated AArch64 source diagnostic reports each guard value and stops before relocation.')
        return
    require(a.reference is not None, '--reference is required for complete image validation')
    paths = [out / name for name in ['loader-sdk-profile.json', 'loader.img', 'native-startup-preflight.Image',
             'native-startup-preflight.Image.lz4', 'fastboot.img', 'report.txt', 'loader-relocated-build/loader.elf',
             'loader-checkpoint-source/src/aarch64/crt0.S']]
    paths += [a.reference, ROOT / '.cache/pixel-sunfish/live-runtime.dtb',
              ROOT / '.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb',
              ROOT / '.cache/sunfish-mobian-comparison/artifacts/sm7150-google-sunfish.dtb']
    kernel = out / 'sdk/board/sunfish/debug/elf/sel4.elf'
    paths.append(kernel)
    hashes = {str(p.resolve()): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    layout = module('validate-image-boot-layout')
    memory = module('audit-capdl-memory')
    sdk = module('prepare-image-loader-sdk')
    pack = module('repack-boot-v2')
    profile = json.loads((out / 'loader-sdk-profile.json').read_text())
    require(profile['native_startup_entry_guard'] and profile['base_sdk_unchanged']
            and profile['kernel_initialiser_monitor_unchanged']
            and not profile['task_images'] and profile['entry_interrupt_masked'], 'closed native loader profile')
    require(profile['loader_link_address'] == '0xc0000000'
            and profile['expected_runtime_source'] == '0xb0081000'
            and profile['native_startup_max_relocation_bytes'] == 0x6000000, 'exact runtime source/envelope guard')
    code = (out / 'loader-checkpoint-source/src/aarch64/crt0.S').read_text()
    require(code.index('ldr x1, =0xb0081000') < code.index('    bl     relocate'), 'source guard before relocation')
    if profile.get('native_source_diagnostic', False):
        validate_source_diagnostic(profile, out)
    else:
        require('    b.ne .Lnative_source_fail' in code and '    b.cs .Lnative_source_fail' in code
                and '    b.hi .Lnative_source_fail' in code and 'IMAGE SOURCE STOP' in code,
                'runtime guard branches')
    require(profile['rebuilt_baseline_loadable_segments_match']
            and profile['all_objects_rebuilt_from_private_board_source'], 'source-built baseline parity')
    segments = sdk.segments(out / 'loader-relocated-build/loader.elf')
    require(len(segments) == 1 and segments[0][:2] == (0xc0000000, 0xc0000000), 'private loader ELF')
    payload = (out / 'loader.img').read_bytes()
    image = (out / 'native-startup-preflight.Image').read_bytes()
    require(image[4096:] == payload and len(payload) <= 0x6000000, 'complete Image binding/envelope')
    require(payload[:64] == segments[0][3][:64], 'loader entry from actual ELF')
    header_at = segments[0][2]
    require(header_at % 8 == 0 and header_at <= 0x20000, 'loader prefix bound/alignment')
    header = struct.unpack_from('<8Q', payload, header_at)
    magic, size, entry, initial_start, initial_end, pv, ve, count = header
    require(magic == 0x5e14dead14de5ead and size + header_at == len(payload)
            and 0 < count <= 64 and initial_end > initial_start, 'complete loader metadata')
    targets = []
    data_start = header_at + 64 + count * 32
    for i in range(count):
        start, length, offset, kind = struct.unpack_from('<4Q', payload, header_at + 64 + i * 32)
        require(kind == 1 and length > 0 and data_start + offset + length <= len(payload), 'boot copy record')
        targets.append((start, length))
    # seL4 returns its .boot pages as normal untyped RAM after boot. CapDL
    # legitimately reuses those pages later; live kernel text/data and the
    # initialiser must remain disjoint from every application frame.
    kernel_segments = sdk.segments(kernel)
    symbols = subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-readelf', '-sW', str(kernel)], text=True)
    boot_end = re.findall(r'^\s*\d+: ([0-9a-fA-F]+)\s+\d+ NOTYPE\s+GLOBAL\s+DEFAULT\s+\d+ ki_boot_end$', symbols, re.M)
    require(len(boot_end) == 1, 'exact kernel boot-reclaim symbol')
    kv, kp, kn, _ = kernel_segments[0]
    reclaimed = (kp, int(boot_end[0], 16) - kv)
    require(reclaimed == targets[0] and reclaimed[1] == kn and kn > 0, 'exact reclaimed boot segment')
    live = ROOT / '.cache/pixel-sunfish/live-runtime.dtb'
    cells = memory.read_cells(live, '/memory', 'reg')
    require(cells and len(cells) % 4 == 0, 'cached RAM cells')
    ram = [((cells[i] << 32) | cells[i+1], (cells[i+2] << 32) | cells[i+3]) for i in range(0, len(cells), 4)]
    for dt in (live, memory.DEFAULT_DTB, ROOT / '.cache/sunfish-mobian-comparison/artifacts/sm7150-google-sunfish.dtb'):
        reserved, _ = memory.read_regions(dt)
        # Audit the entire permitted envelope, not merely today's image size.
        layout.validate_range('runtime-guarded source envelope', 0xb0080000, 0x6001000, ram, reserved)
        layout.validate_range('runtime-guarded relocated envelope', 0xc0000000, 0x6000000, ram, reserved)
        layout.validate_range('complete initial task', initial_start, initial_end - initial_start, ram, reserved)
        for start, length in targets:
            layout.validate_range('actual copy destination', start, length, ram, reserved)
    layout.validate_disjoint(0xb0080000, len(image), 0xc0000000, len(payload))
    for i, (start, length) in enumerate(targets):
        layout.validate_disjoint(start, length, 0xc0000000, len(payload))
        layout.validate_disjoint(start, length, 0xb0080000, len(image))
        for other, size in targets[i+1:]:
            layout.validate_disjoint(start, length, other, size)
    reused_boot_frames = 0
    for start, length, name in memory.read_pages(out / 'report.txt'):
        layout.validate_disjoint(start, length, 0xc0000000, len(payload))
        layout.validate_disjoint(start, length, initial_start, initial_end - initial_start)
        for destination, size in targets:
            if (destination, size) == reclaimed and destination <= start and start + length <= destination + size:
                reused_boot_frames += 1
                continue
            layout.validate_disjoint(start, length, destination, size)
    _, packed, _ = pack.components((out / 'fastboot.img').read_bytes())
    _, reference, _ = pack.components(a.reference.read_bytes())
    require(packed['dtb'] == reference['dtb'] and packed['ramdisk'] == reference['ramdisk'], 'preserved reference DTB/ramdisk')
    require(packed['kernel'] == (out / 'native-startup-preflight.Image.lz4').read_bytes(), 'packed native kernel binding')
    require((out / 'fastboot.img').stat().st_size <= 0x10000000, 'device download limit')
    result = dict(source_image_base='0xb0080000', source_placement_checked_before_relocation=True,
                  unexpected_source_stops_before_copy=True, relocated_loader_base='0xc0000000',
                  loader_bytes=len(payload), initial_task_start=hex(initial_start), initial_task_end=hex(initial_end),
                  copy_targets=[[hex(a), hex(b)] for a, b in targets],
                  reclaimed_kernel_boot_range=[hex(v) for v in reclaimed],
                  reclaimed_kernel_boot_frames=reused_boot_frames,
                  full_envelopes_and_copy_targets_avoid_three_dt_reservations=True,
                  base_kernel_monitor_and_initialiser_unchanged=True,
                  scope='Offline range/packing audit plus compiled runtime source guard. Device boot and actual MPSS authentication remain unproved.')
    for path, expected in hashes.items():
        require(hashlib.sha256(Path(path).read_bytes()).hexdigest() == expected, 'audit input changed')
    result['input_sha256'] = hashes
    (out / 'boot-layout-audit.json').write_text(json.dumps(result, indent=2) + '\n')
    print('PASS: complete large-image source, relocation, initial task, copy targets and preserved boot components.')


if __name__ == '__main__':
    main()
