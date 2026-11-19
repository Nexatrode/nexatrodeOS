#!/usr/bin/env python3
"""Audit the actual post-ready loader payload without packaging a boot image."""
from pathlib import Path
import argparse
import copy
import hashlib
import importlib.util
import json
import re
import struct
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def require(value, text):
    if not value:
        raise ValueError(text)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def runtime_source_bounds(prepare):
    """Return the measured runtime entry and enclosing Fastboot Image base."""
    runtime_source = prepare.RUNTIME_SOURCE
    return runtime_source - 0x1000, runtime_source


def audit(context):
    layout, memory = context['layout'], context['memory']
    source_base, runtime_source = runtime_source_bounds(context['sdk'])
    payload, segment, header = context['payload'], context['segment'], context['header']
    require(segment[:2] == (0xc0000000, 0xc0000000), 'exact relocated loader ELF')
    require(payload[:len(segment[3])] == segment[3], 'complete compiled loader code binding')
    require(0 < len(payload) <= 0x6000000, 'runtime source guard payload envelope')
    header_at = segment[2]
    require(header_at % 8 == 0 and header_at <= 0x20000, 'guarded loader prefix alignment/size')
    magic, size, entry, initial_start, initial_end, pv, ve, count = header
    require(magic == 0x5e14dead14de5ead and size + header_at == len(payload)
            and 0 < count <= 64 and initial_end > initial_start, 'complete loader metadata')
    require(entry == context['kernel_entry'], 'pinned kernel virtual entry')
    targets = context['targets']
    require(len(targets) == count, 'complete copy table')
    data_start = header_at + 64 + count * 32
    source_spans = []
    for start, length, offset, kind in targets:
        require(kind == 1 and length > 0 and data_start + offset + length <= len(payload), 'copy payload range')
        source_spans.append((data_start + offset, length))
    for i, (start, length) in enumerate(source_spans):
        for other, size in source_spans[i+1:]:
            layout.validate_disjoint(start, length, other, size)
    reclaimed = context['reclaimed']
    require(tuple(targets[0][:2]) == reclaimed, 'exact reclaimed kernel boot copy')
    ram = context['ram']
    for reserved in context['reservations']:
        # Reuse the previously compiled runtime source guard. Audit its full
        # allowed envelopes rather than assuming today's payload length.
        layout.validate_range('runtime-guarded source envelope', source_base, 0x6001000, ram, reserved)
        layout.validate_range('runtime-guarded relocated envelope', 0xc0000000, 0x6000000, ram, reserved)
        layout.validate_range('complete initial task', initial_start, initial_end - initial_start, ram, reserved)
        for start, length, _, _ in targets:
            layout.validate_range('actual boot-copy destination', start, length, ram, reserved)
    layout.validate_disjoint(source_base, len(payload) + 4096, 0xc0000000, len(payload))
    for i, (start, length, _, _) in enumerate(targets):
        layout.validate_disjoint(start, length, source_base, len(payload) + 4096)
        layout.validate_disjoint(start, length, 0xc0000000, len(payload))
        for other, size, _, _ in targets[i+1:]:
            layout.validate_disjoint(start, length, other, size)
    reused = 0
    for start, length, name in context['frames']:
        layout.validate_disjoint(start, length, 0xc0000000, len(payload))
        layout.validate_disjoint(start, length, initial_start, initial_end - initial_start)
        for destination, size, _, _ in targets:
            if (destination, size) == reclaimed and destination <= start and start + length <= destination + size:
                reused += 1
                continue
            layout.validate_disjoint(start, length, destination, size)
    return dict(loader_bytes=len(payload), header_offset=header_at, copy_target_count=count,
                initial_task_start=hex(initial_start), initial_task_end=hex(initial_end),
                source_guard_base=hex(runtime_source), relocated_loader_base='0xc0000000',
                reclaimed_kernel_boot_range=[hex(v) for v in reclaimed], reclaimed_frames=reused,
                source_and_destination_envelope_bytes=0x6000000,
                all_actual_frames_and_copy_targets_checked=True, all_three_dt_reservations_checked=True,
                source_guard_mode=context['source_guard_mode'])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output-dir', type=Path, required=True)
    p.add_argument('--loader-reference', type=Path, required=True)
    a = p.parse_args()
    out, reference = a.output_dir.resolve(), a.loader_reference.resolve()
    layout, memory, sdk = load('validate-image-boot-layout'), load('audit-capdl-memory'), load('prepare-image-loader-sdk')
    dtbs = [ROOT / '.cache/pixel-sunfish/live-runtime.dtb',
            ROOT / '.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb',
            ROOT / '.cache/sunfish-mobian-comparison/artifacts/sm7150-google-sunfish.dtb']
    kernel = reference / 'sdk/board/sunfish/debug/elf/sel4.elf'
    paths = [out / 'loader.img', out / 'capdl.json', out / 'report.txt', out / 'native_startup_preflight.elf',
             out / 'native-startup-mpss-runtime.system', out / 'integration-result.json',
             reference / 'loader-sdk-profile.json', reference / 'loader-relocated-build/loader.elf',
             reference / 'loader-checkpoint-source/src/aarch64/crt0.S', kernel, Path(__file__).resolve(),
             HERE / 'validate-image-boot-layout.py', HERE / 'audit-capdl-memory.py', HERE / 'prepare-image-loader-sdk.py'] + dtbs
    hashes = {str(q): sha(q) for q in paths}
    proof = json.loads((out / 'integration-result.json').read_text())
    require(proof['result'] == 'passed' and proof['root_elf_sha256'] == sha(out / 'native_startup_preflight.elf'),
            'actual compiled isolated root binding')
    profile = json.loads((reference / 'loader-sdk-profile.json').read_text())
    require(profile['native_startup_entry_guard'] and profile['base_sdk_unchanged']
            and profile['kernel_initialiser_monitor_unchanged'] and profile['entry_interrupt_masked']
            and not profile['task_images'] and profile['loader_link_address'] == '0xc0000000'
            and profile['expected_runtime_source'] == hex(sdk.RUNTIME_SOURCE)
            and profile['source_image_base'] == hex(sdk.RUNTIME_SOURCE - 0x1000)
            and profile['native_startup_max_relocation_bytes'] == 0x6000000, 'preserved compiled native loader contract')
    asm = (reference / 'loader-checkpoint-source/src/aarch64/crt0.S').read_text()
    require(asm.index(f'ldr x1, ={hex(sdk.RUNTIME_SOURCE)}') < asm.index('    bl     relocate'),
            'actual source guard precedes copy')
    if profile.get('native_source_report_guard', False):
        require('    b.ne .Lnative_source_address_fail' in asm
                and '    b.hi .Lnative_source_prefix_fail' in asm
                and '    b.cs .Lnative_source_overflow_fail' in asm
                and '    b.hi .Lnative_source_extent_fail' in asm
                and '    ldr x1, =0x80081000' in asm
                and '    ldr x1, =0x85700000' in asm
                and '    b.hi .Lnative_source_reservation_fail' in asm
                and '.asciz "SOURCE RANGE END"' in asm
                and '    b .Lnative_source_continue' in asm
                and '.Lnative_source_continue:' in asm
                and 'IMAGE SOURCE STOP' not in asm,
                'labeled source failures stop and both source-range checks gate continuation')
        source_guard_mode = 'labeled failures stop; pass continues to relocation'
    elif profile.get('native_source_diagnostic', False):
        require('    b.ne .Lnative_source_address_fail' in asm
                and '    b.hi .Lnative_source_prefix_fail' in asm
                and '    b.cs .Lnative_source_overflow_fail' in asm
                and '    b.hi .Lnative_source_extent_fail' in asm
                and '    b .Lnative_source_diagnostic_stop' in asm
                and 'IMAGE SOURCE STOP' not in asm,
                'standalone labeled source diagnostic stops before copy')
        source_guard_mode = 'labeled source diagnostic stops before relocation'
    else:
        require('    b.ne .Lnative_source_fail' in asm
                and '    b.cs .Lnative_source_fail' in asm
                and '    b.hi .Lnative_source_fail' in asm
                and 'IMAGE SOURCE STOP' in asm,
                'actual source guard failure path')
        source_guard_mode = 'collapsed source failure label'
    payload = (out / 'loader.img').read_bytes()
    segments = sdk.segments(reference / 'loader-relocated-build/loader.elf')
    require(len(segments) == 1, 'single retained loader segment')
    segment = segments[0]
    header = list(struct.unpack_from('<8Q', payload, segment[2]))
    require(0 < header[7] <= 64, 'bounded metadata before reading records')
    targets = [list(struct.unpack_from('<4Q', payload, segment[2] + 64 + 32*i)) for i in range(header[7])]
    kernel_segments = sdk.segments(kernel)
    symbols = subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-readelf', '-sW', str(kernel)], text=True)
    boot_end = re.findall(r'^\s*\d+: ([0-9a-fA-F]+)\s+\d+ NOTYPE\s+GLOBAL\s+DEFAULT\s+\d+ ki_boot_end$', symbols, re.M)
    require(len(boot_end) == 1, 'one pinned boot-reclaim symbol')
    kv, kp, kn, _ = kernel_segments[0]
    reclaimed = (kp, int(boot_end[0], 16) - kv)
    require(reclaimed == (0x80000000, 0x10000) and kn == reclaimed[1], 'only the pinned .boot segment is reclaimable')
    kernel_entry = struct.unpack_from('<Q', kernel.read_bytes(), 24)[0]
    cells = memory.read_cells(dtbs[0], '/memory', 'reg')
    require(cells and len(cells) % 4 == 0, 'live RAM cells')
    ram = [((cells[i] << 32) | cells[i+1], (cells[i+2] << 32) | cells[i+3]) for i in range(0, len(cells), 4)]
    context = dict(layout=layout, memory=memory, sdk=sdk, payload=payload, segment=segment, header=header, targets=targets,
                   kernel_entry=kernel_entry, reclaimed=reclaimed, ram=ram,
                   reservations=[memory.read_regions(dt)[0] for dt in dtbs], frames=memory.read_pages(out / 'report.txt'),
                   source_guard_mode=source_guard_mode)
    result = audit(context)
    mutations = []

    def reject(label, changed):
        try:
            audit(changed)
        except ValueError:
            mutations.append(label)
        else:
            raise ValueError('layout mutation accepted: ' + label)

    for target in range(len(targets)):
        for address, label in [(sdk.RUNTIME_SOURCE, 'source payload'), (0xc0000000, 'relocated payload'),
                               (0x8b000000, 'MPSS reserved target'), (0x9d600000, 'display tail'),
                               (0x85d00000, 'reference XBL')]:
            changed = dict(context, targets=copy.deepcopy(targets))
            changed['targets'][target][0] = address
            reject(f'copy {target} into {label}', changed)
    changed = dict(context, header=header.copy())
    changed['header'][3:5] = [0x8b000000, 0x8b001000]
    reject('initial task in MPSS reservation', changed)
    changed = dict(context, frames=context['frames'] + [(0x80010000, 4096, 'bad_live_kernel_frame')])
    reject('application frame in live kernel', changed)
    changed = dict(context, frames=context['frames'] + [(header[3], 4096, 'bad_initialiser_frame')])
    reject('application frame in initial task', changed)
    changed = dict(context, reclaimed=(0x80000000, 0x20000))
    reject('overbroad kernel reclaim exception', changed)
    for path, expected in hashes.items():
        require(sha(Path(path)) == expected, 'layout audit input changed')
    result.update(input_sha256=hashes, mutations_rejected=mutations,
                  scope='Actual runtime loader payload and CapDL allocation ranges only. The freezer separately verifies Android-v2 packaging; device execution is not proven.',
                  android_v2_boot_image_packaging_audited=False,
                  predecessor_auth_evidence_bound=proof.get('predecessor_auth_evidence_bound') is True,
                  physical_authentication_proven=False, physical_glink_ready_proven=False,
                  native_wifi_proven=False, ready_for_device_boot=False, device_actions=[])
    if result['predecessor_auth_evidence_bound'] is not True:
        raise ValueError('post-auth runtime layout requires the prior PAS4/Q6-ready evidence binding')
    (out / 'runtime-loader-layout-audit.json').write_text(json.dumps(result, indent=2) + '\n')
    print('PASS: actual runtime loader/copy/frame ranges, ' + str(len(mutations)) + ' rejected mutations; no boot image or device action.')


if __name__ == '__main__':
    main()
