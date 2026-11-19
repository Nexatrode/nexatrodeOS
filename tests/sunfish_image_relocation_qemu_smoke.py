"""Execute an actual loader entry/relocation hold; QEMU is not Pixel evidence.

Run with python3 -m tests.sunfish_image_relocation_qemu_smoke OUT_DIR.
"""
import argparse
import json
from pathlib import Path
import re
import socket
import struct
import subprocess
import tempfile
import time

from tests.sunfish_entry_hold_qemu_smoke import packet


def qmp(connection, command):
    connection.write(json.dumps(command).encode() + b'\n')
    connection.flush()
    while True:
        result = json.loads(connection.readline())
        if 'error' in result:
            raise RuntimeError(result)
        if 'return' in result:
            return result['return']


def save_physical(endpoint, address, size, path):
    with socket.socket(socket.AF_UNIX) as monitor:
        monitor.settimeout(30)
        monitor.connect(str(endpoint))
        stream = monitor.makefile('rwb')
        assert 'QMP' in json.loads(stream.readline())
        qmp(stream, {'execute': 'qmp_capabilities'})
        result = qmp(stream, {'execute': 'human-monitor-command', 'arguments': {
            'command-line': 'pmemsave 0x%x %d "%s"' % (address, size, path)}})
        assert result == '', result


def decode_input_rows(content, buffer, count=9):
    header = (Path(__file__).resolve().parents[1] / 'platforms/sunfish/loader-boot-checkpoints.h').read_text()
    table = header.split('checkpoint_font[36][7] = {', 1)[1].split('};', 1)[0]
    patterns = [tuple(int(n) for n in match.split(','))
                for match in re.findall(r'\{([0-9, ]+)\}', table)]
    assert len(patterns) == 36
    alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'
    glyphs = dict(zip(patterns, alphabet))
    result = []
    for row in range(count):
        text = ''
        for column in range(24):
            pattern = []
            for gy in range(7):
                bits = 0
                for gx in range(5):
                    pixel = ((buffer * 2340 + 200 + row * 96 + gy * 7 + 3) * 1080 +
                             48 + column * 42 + gx * 7 + 3) * 4
                    value = int.from_bytes(content[pixel:pixel + 4], 'little')
                    assert value in [0x00122b52, 0x0000e5e5], hex(value)
                    bits = (bits << 1) | (value == 0x0000e5e5)
                pattern.append(bits)
            text += glyphs[tuple(pattern)] if any(pattern) else ' '
        result.append(text.rstrip())
    return result


def run(out, image_base=None, reject_copy=False, reject_task_plan=False):
    profile = json.loads((out / 'loader-sdk-profile.json').read_text())
    before = profile.get('hold_before_relocation', False)
    copied = profile.get('hold_after_copy', False)
    inputs = profile.get('copy_inputs', False)
    relocation_progress = profile.get('relocation_progress', False)
    tasks = profile.get('task_images', False)
    boot = profile.get('kernel_start', profile.get('boot_diagnostic', False))
    kernel_handoff = boot and not (reject_copy or reject_task_plan)
    assert not reject_task_plan or (tasks and not reject_copy)
    assert not reject_copy or relocation_progress, 'guard injection requires the returning-copy relocation diagnostic'
    expected_source = int(profile.get('expected_runtime_source','0x80081000'),0)
    default_image_base = int(profile.get('source_image_base','0x80080000'),0)
    image_base = default_image_base if image_base is None else image_base
    assert image_base == default_image_base or inputs, 'alternate source placement is only tested by the read-only snapshot'
    assert sum([before, copied, profile['hold_after_relocation']]) == 1
    assert profile['all_objects_rebuilt_from_private_board_source']
    elf = out / 'loader-relocated-build/loader.elf'
    symbols = {}
    for line in subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm', '-n', str(elf)], text=True).splitlines():
        address, _, name = line.split()
        symbols[name] = int(address, 16)
    base = symbols['_start']
    hold = symbols['loader_copy_hold_stop' if copied or reject_copy else ('loader_task_images_stop' if tasks else ('loader_entry_hold' if before else 'loader_relocated_hold'))]
    source_base = image_base + 4096
    runtime_base = source_base if before or copied or reject_copy else base
    runtime_hold = runtime_base + hold - base
    image = bytearray((out / 'loader.img').read_bytes())
    immutable_prefix = symbols['_bss'] - base
    payload_offset = symbols['_bss_end'] - base
    header = struct.unpack_from('<8Q',image,payload_offset)
    if kernel_handoff:
        runtime_hold = header[2]
    task_regions = [struct.unpack_from('<4Q',image,payload_offset+64+i*32) for i in range(11)] if tasks else []
    task_data_start = payload_offset+64+11*32
    assert image[hold - base:hold - base + 4] == b'\0\0\0\x14'
    with tempfile.TemporaryDirectory(prefix='sunfish-relocation-') as temp:
        temp = Path(temp)
        endpoint = temp / 'gdb.sock'
        qmp_endpoint = temp / 'qmp.sock'
        dump = temp / 'relocated.bin'
        with open(temp / 'qemu.log', 'w+') as log:
            process = subprocess.Popen([
                'qemu-system-aarch64', '-M', 'virt', '-cpu', 'cortex-a55',
                '-accel', 'tcg', '-m', '3G', '-display', 'none', '-serial', 'none',
                '-monitor', 'none', '-S', '-gdb', 'unix:%s,server=on,wait=off' % endpoint,
                '-qmp', 'unix:%s,server=on,wait=off' % qmp_endpoint,
                '-device', 'loader,file=%s,addr=0x%x,cpu-num=0,force-raw=on' % (out / 'modem-image-probe.Image', image_base),
            ], stdout=log, stderr=log)
            try:
                for _ in range(100):
                    if process.poll() is not None:
                        log.seek(0)
                        raise RuntimeError(log.read())
                    if endpoint.exists() and qmp_endpoint.exists():
                        break
                    time.sleep(0.05)
                with socket.socket(socket.AF_UNIX) as connection:
                    connection.settimeout(30)
                    connection.connect(str(endpoint))
                    breakpoints = [runtime_hold] + ([] if tasks else [symbols['main']])
                    if tasks and not kernel_handoff:
                        breakpoints.extend(symbols[name] for name in ['start_kernel', 'arch_mmu_enable', 'plat_start_cpu'] if name in symbols)
                    elif kernel_handoff:
                        # Execute the actual MMU setup and branch, stopping at
                        # the first seL4 instruction before platform MMIO.
                        breakpoints.extend([hold, symbols['plat_start_cpu']])
                    if before:
                        breakpoints.append(runtime_base + symbols['relocate'] - base)
                    if copied:
                        breakpoints.extend(runtime_base + symbols[name] - base
                                           for name in ['flush_dcache', 'invalidate_icache', 'relocation_log'])
                        breakpoints.extend([base, runtime_base + symbols['main'] - base])
                        if profile.get('copy_progress', False) or inputs:
                            breakpoints.append(runtime_base + symbols['memmove'] - base)
                    stages = []
                    if relocation_progress:
                        breakpoints.extend(source_base + symbols[name] - base
                                           for name in ['main', 'memmove', 'relocation_log'])
                        stages = [source_base + symbols[name] - base
                                  for name in ['flush_dcache', 'invalidate_icache']] + [base]
                        if tasks and not reject_copy:
                            stages.append(symbols['main'])
                        if kernel_handoff:
                            stages.extend(symbols[name] for name in ['start_kernel', 'arch_mmu_enable', 'arch_jump_to_kernel'])
                        breakpoints.extend(stages)
                        if reject_copy:
                            injection = source_base + symbols['loader_copy_progress_resume'] - base
                            breakpoints.append(injection)
                        if reject_task_plan:
                            task_injection = symbols['main']
                    for address in breakpoints:
                        assert packet(connection, 'Z0,%x,4' % address) == 'OK'
                    # An old input snapshot can deliberately reproduce a bad
                    # overlapping placement. Never overwrite its source with
                    # the destination's trailing canary.
                    canaries = [address for address in [base - 4, base + len(image)]
                                if not (image_base <= address < image_base + len(image) + 4096)]
                    if tasks:
                        for start,size,_,_ in task_regions:
                            for address in [start-4,start+size]:
                                if not any(address < other+length and other < address+4 for other,length,_,_ in task_regions):
                                    canaries.append(address)
                        canaries = sorted(set(canaries))
                    for address in canaries:
                        assert packet(connection, 'M%x,4:a5a5a5a5' % address) == 'OK'
                    if before or inputs or reject_copy:
                        destination_before = temp / 'destination-before.bin'
                        save_physical(qmp_endpoint, base, len(image), destination_before)
                    source_bss_first = max(0, image_base + 4096 + immutable_prefix - base)
                    source_bss_last = min(len(image), image_base + 4096 + payload_offset - base)
                    source_bss_in_destination = source_bss_first < source_bss_last
                    observed_stages = []
                    expected_stops = ([injection] if reject_copy else stages) + [runtime_hold]
                    for expected in expected_stops:
                        try:
                            stopped = packet(connection, 'c')
                        except socket.timeout:
                            with socket.socket(socket.AF_UNIX) as monitor:
                                monitor.settimeout(5);monitor.connect(str(qmp_endpoint));stream=monitor.makefile('rwb')
                                assert 'QMP' in json.loads(stream.readline());qmp(stream,{'execute':'qmp_capabilities'})
                                qmp(stream,{'execute':'stop'})
                                registers_text=qmp(stream,{'execute':'human-monitor-command','arguments':{'command-line':'info registers'}})
                            raise RuntimeError('Timeout waiting for %s after %s; %s'%(hex(expected),observed_stages,registers_text))
                        assert stopped.startswith(('T05', 'S05')), stopped
                        registers = bytes.fromhex(packet(connection, 'g'))
                        pc = int.from_bytes(registers[32 * 8:33 * 8], 'little')
                        assert pc == expected, ('unexpected stop', hex(pc), 'expected', hex(expected))
                        if reject_copy and expected == injection:
                            # Invalid total at the real C entry must stop at
                            # failure, without reaching caches or destination.
                            assert packet(connection, 'P4=0100000500000000') == 'OK'
                        elif reject_task_plan and expected == task_injection:
                            observed_stages.append(hex(pc))
                            table = base+payload_offset
                            address = table+64+10*32
                            assert packet(connection,'M%x,8:0000008b00000000'%address)=='OK'
                            struct.pack_into('<Q',image,payload_offset+64+10*32,0x8b000000)
                        elif expected != runtime_hold:
                            observed_stages.append(hex(pc))
                            if not kernel_handoff or expected in stages[:4]:
                                x19 = int.from_bytes(registers[19 * 8:20 * 8], 'little')
                                assert x19 == base, ('destination return register lost', hex(x19))
                        if expected != runtime_hold:
                            assert packet(connection, 'z0,%x,4' % expected) == 'OK'
                            breakpoints.remove(expected)
                    pstate = int.from_bytes(registers[33 * 8:33 * 8 + 4], 'little')
                    if before or copied or relocation_progress:
                        assert pstate & 0x3c0 == 0x3c0, hex(pstate)
                    if relocation_progress and not reject_copy:
                        sp = int.from_bytes(registers[31 * 8:32 * 8], 'little')
                        assert base + immutable_prefix <= sp <= base + payload_offset
                    if kernel_handoff:
                        args = [int.from_bytes(registers[i*8:(i+1)*8], 'little') for i in range(6)]
                        assert args == list(header[3:7])+[0,0], ('kernel arguments',args)
                        assert pstate & 15 == 5, ('kernel must start in EL1h', hex(pstate))
                    for address in breakpoints:
                        assert packet(connection, 'z0,%x,4' % address) == 'OK'
                    for address in canaries:
                        value = temp / 'canary.bin'
                        save_physical(qmp_endpoint,address,4,value)
                        assert value.read_bytes() == b'\xa5'*4
                    with socket.socket(socket.AF_UNIX) as monitor:
                        monitor.settimeout(30)
                        monitor.connect(str(qmp_endpoint))
                        stream = monitor.makefile('rwb')
                        assert 'QMP' in json.loads(stream.readline())
                        qmp(stream, {'execute': 'qmp_capabilities'})
                        result = qmp(stream, {'execute': 'human-monitor-command', 'arguments': {
                            'command-line': 'pmemsave 0x%x %d "%s"' % (base if copied and not inputs else runtime_base, len(image), dump)}})
                        assert result == '', result
                        if before or inputs or reject_copy:
                            untouched = temp / 'untouched-destination.bin'
                            result = qmp(stream, {'execute': 'human-monitor-command', 'arguments': {
                                'command-line': 'pmemsave 0x%x %d "%s"' % (base, len(image), untouched)}})
                            assert result == '', result
                            content = untouched.read_bytes()
                            previous = destination_before.read_bytes()
                            assert len(content) == len(image)
                            if source_bss_in_destination:
                                # With an intentionally bad overlap, the live
                                # source stack itself aliases the destination.
                                assert content[:source_bss_first] == previous[:source_bss_first]
                                assert content[source_bss_last:] == previous[source_bss_last:]
                            else:
                                assert content == previous, 'relocation destination was modified'
                        if inputs or reject_copy or relocation_progress:
                            framebuffer = temp / 'framebuffer.bin'
                            result = qmp(stream, {'execute': 'human-monitor-command', 'arguments': {
                                'command-line': 'pmemsave 0x9c000000 %d "%s"' % (2 * 1080 * 2340 * 4, framebuffer)}})
                            assert result == '', result
                            expected_rows = ['IMAGE COPY INPUTS', 'SRC %016X' % runtime_base,
                                             'DST %016X' % base, 'PRE %016X' % immutable_prefix,
                                             'PAY %016X' % payload_offset, 'LEN %016X' % len(image),
                                             'ERR %016X' % ((runtime_base != expected_source) |
                                                 (512 if runtime_base < base + len(image) and base < runtime_base + len(image) else 0)),
                                             'NO COPY OR KERNEL', 'RESTART WHEN DONE']
                            if reject_copy:
                                expected_rows = ['IMAGE COPY FAIL FFFFFFFE']
                            elif tasks:
                                expected_rows = ['IMAGE LOAD FAIL C00001A0' if reject_task_plan else ('IMAGE KERNEL' if kernel_handoff else 'IMAGE LOAD PASS')]
                            elif relocation_progress:
                                expected_rows = ['IMAGE RELOCATED']
                            for buffer in range(2):
                                rows = decode_input_rows(framebuffer.read_bytes(), buffer, len(expected_rows))
                                assert rows == expected_rows, (rows, expected_rows)
                        if tasks and not reject_copy:
                            for i,(start,size,offset,_) in enumerate(task_regions):
                                loaded = temp / ('task-%d.bin'%i)
                                result = qmp(stream, {'execute':'human-monitor-command','arguments':{
                                    'command-line':'pmemsave 0x%x %d "%s"'%(start,size,loaded)}})
                                assert result=='',result
                                data = loaded.read_bytes()
                                if reject_task_plan:
                                    assert len(data)==size and not any(data), 'target written before whole plan validation'
                                else:
                                    assert data==image[task_data_start+offset:task_data_start+offset+size], ('task image differs',i)
                    relocated = dump.read_bytes()
                    assert len(relocated) == len(image)
                    assert relocated[:immutable_prefix] == image[:immutable_prefix], 'relocated code/data mismatch'
                    assert relocated[payload_offset:] == image[payload_offset:], 'relocated full payload mismatch'
                    # IMAGE RELOCATED starts with I: check its foreground and background.
                    for buffer in ([] if kernel_handoff else range(2)):
                        fb = 0x9c000000 + buffer * 1080 * 2340 * 4
                        for x, y, expected in [(0, 160, 0x00122b52), (55, 200, 0x0000e5e5)]:
                            address = fb + (y * 1080 + x) * 4
                            value = bytes.fromhex(packet(connection, 'm%x,4' % address))
                            assert int.from_bytes(value, 'little') == expected
                        if copied and not inputs:
                            # P at column 11 distinguishes COPY PASS from
                            # COPY FAIL/CHECK/START/SKIP on the final screen.
                            for y, expected in [(207, 0x0000e5e5), (200, 0x00122b52)]:
                                address = fb + (y * 1080 + 538) * 4
                                value = bytes.fromhex(packet(connection, 'm%x,4' % address))
                                assert int.from_bytes(value, 'little') == expected, 'copy reporter did not display PASS'
                    return {'pc': hex(pc), 'relocation_hold_reached': not (before or copied or reject_copy or tasks),
                            'task_images_hold_reached':tasks and not (reject_copy or kernel_handoff),
                            'task_plan_rejection_reached':reject_task_plan,
                            'task_regions_compared':len(task_regions) if tasks and not reject_copy else 0,
                            'task_regions_match':tasks and not reject_task_plan and not reject_copy,
                            'task_regions_untouched_after_bad_plan':reject_task_plan,
                            'kernel_start_reached':kernel_handoff,
                            'real_kernel_entry_reached':kernel_handoff,
                            'kernel_first_instruction_executed':False,
                            'kernel_arguments_match':kernel_handoff,
                            'mmu_enabled_handoff_reached':kernel_handoff,
                            'secondary_cpu_start_reached':False,
                            'entry_hold_reached': before, 'interrupt_mask': hex(pstate & 0x3c0),
                            'copy_hold_reached': copied and not inputs,
                            'reported_copy_pass': (copied and not inputs) or (relocation_progress and not reject_copy),
                            'copy_guard_rejection_reached': reject_copy,
                            'copy_input_hold_reached': inputs, 'input_rows': expected_rows if inputs else None,
                            'final_screen_rows': expected_rows if inputs or reject_copy or relocation_progress else None,
                            'image_load_base': hex(image_base),
                            'expected_runtime_source': hex(expected_source),
                            'aligned_ram_copy_with_progress': profile.get('copy_progress', False) or relocation_progress,
                            'observed_cache_and_destination_entry_pcs': observed_stages,
                            'cache_maintenance_reached': not (before or copied or reject_copy),
                            'destination_code_executed': not (before or copied or reject_copy),
                            'relocation_destination_untouched': (before or inputs or reject_copy) and not source_bss_in_destination,
                            'source_bss_overlaps_destination': source_bss_in_destination,
                            'destination_unchanged_outside_live_source_bss': before or inputs or reject_copy,
                            'relocated_bytes': 0 if before or inputs or reject_copy else len(image),
                            'immutable_payload_bytes_compared': len(image) - payload_offset,
                            'code_and_data_match': True, 'entire_payload_matches': True,
                            'outer_canaries_preserved': True, 'both_framebuffers_drawn': True,
                            'outer_canaries_checked': len(canaries),
                            'loader_main_not_reached': not tasks or reject_copy, 'uart_stubbed': False,
                            'native_uart_output_suppressed':profile.get('uart_output_suppressed',False),
                            'native_pixel_evidence': False}
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('out_dir', type=Path)
    parser.add_argument('--image-base', type=lambda value: int(value, 0))
    parser.add_argument('--reject-copy', action='store_true')
    parser.add_argument('--reject-task-plan', action='store_true')
    args = parser.parse_args()
    result = run(args.out_dir.resolve(), args.image_base, args.reject_copy,args.reject_task_plan)
    name = ('qemu-kernel-handoff-result.json' if result['real_kernel_entry_reached'] else
            'qemu-task-plan-rejection-result.json' if result['task_plan_rejection_reached'] else
            'qemu-task-images-result.json' if result['task_images_hold_reached'] else
            'qemu-copy-rejection-result.json' if result['copy_guard_rejection_reached'] else
            'qemu-inputs-%x-result.json' % int(result['image_load_base'],0) if result['copy_input_hold_reached'] else
            'qemu-copy-result.json' if result['copy_hold_reached'] else
            ('qemu-entry-result.json' if result['entry_hold_reached'] else 'qemu-relocation-result.json'))
    (args.out_dir / name).write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
