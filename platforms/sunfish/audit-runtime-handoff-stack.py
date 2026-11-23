#!/usr/bin/env python3
"""Check a known nested handoff-frame budget, not whole-program worst-case stack.

Accounts for persistent Microkit entry, live init, runtime construction/begin,
opening/promotion and the large RMTFS transfer frame. Indirect calls, TLS and
other descendants are not certified by this targeted regression check.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

LLVM = Path('/opt/homebrew/opt/llvm/bin')


def frame_from_disassembly(text):
    instructions = []
    for line in text.splitlines():
        match = re.match(r'\s*([0-9a-f]+):\s+(?:[0-9a-f]{8}\s+)?(.+)', line)
        if match:
            instructions.append((int(match[1], 16), match[2].split('//')[0].strip()))
    frame = 0
    probe = None
    skipped = False
    boundary = False
    for index, (address, ins) in enumerate(instructions):
        if re.match(r'(?:bl|blr|ret)\b', ins):
            boundary = True
            break
        save = re.search(r'\[sp, #-(0x[0-9a-f]+)\]!', ins)
        if save:
            frame += int(save[1], 16)
        if re.search(r'\[sp\], #-(?:0x[0-9a-f]+|[0-9]+)', ins):
            raise ValueError('unsupported post-indexed stack allocation')
        target = re.match(r'sub\s+(x\d+), sp, #(0x[0-9a-f]+), lsl #12', ins)
        if target:
            if probe is not None:
                raise ValueError('multiple probe targets require manual review')
            if int(target[2], 16) == 0:
                raise ValueError('zero stack probe target')
            probe = target[1]
            frame += int(target[2], 16) * 4096
        reserve = re.fullmatch(r'sub\s+sp, sp, #(0x[0-9a-f]+)(, lsl #12)?', ins)
        if reserve:
            amount = int(reserve[1], 16) * (4096 if reserve[2] else 1)
            if probe is not None and not skipped and amount == 4096:
                tail = instructions[index+1:index+4]
                if len(tail) != 3 or not re.fullmatch(r'ldr\s+xzr, \[sp\]', tail[0][1]):
                    raise ValueError('unrecognized stack probe load')
                if not re.fullmatch(r'cmp\s+sp, ' + probe, tail[1][1]):
                    raise ValueError('unrecognized stack probe comparison')
                edge = re.match(r'b\.ne\s+0x([0-9a-f]+)(?:\s|$)', tail[2][1])
                if edge is None or int(edge[1], 16) != address:
                    raise ValueError('unrecognized stack probe backedge')
                skipped = True
            else:
                frame += amount
        elif re.match(r'\w+\s+sp,', ins) and not re.fullmatch(r'add\s+sp, sp, #0x[0-9a-f]+(?:, lsl #12)?', ins) and not re.match(r'cmp\s+sp,', ins):
            raise ValueError('unrecognized SP write: ' + ins)
    if not boundary:
        raise ValueError('truncated prefix without call/return boundary')
    if probe is not None and not skipped:
        raise ValueError('probe target without matched page loop')
    return frame


def choose_init_symbol(names):
    direct = 'rust_sunfish_native_startup_preflight::init'
    wrapper = '<' + direct + ' as core::ops::function::FnOnce<()>>::call_once'
    exact = lambda base: [n for n in names if re.fullmatch(re.escape(base) + r'(?: \(\.llvm\.[0-9]+\))?', n)]
    functions, wrappers = exact(direct), exact(wrapper)
    if len(functions) > 1 or len(wrappers) > 1 or not (functions or wrappers):
        raise ValueError('missing or ambiguous exact initialization implementation')
    if functions:
        return functions[0], wrappers[0] if wrappers else None
    return wrappers[0], None  # init body inlined into the actual call_once frame


def zero_frame_init_thunk(text, destination):
    instructions = []
    for line in text.splitlines():
        match = re.match(r'\s*([0-9a-f]+):\s+(?:[0-9a-f]{8}\s+)?(.+)', line)
        if match:
            instructions.append(match[2].split('//')[0].strip())
    if len(instructions) != 2 or not re.fullmatch(r'mov\s+x8, x0', instructions[0]):
        raise ValueError('outlined initialization wrapper requires reviewed zero-frame thunk')
    branch = re.match(r'b\s+0x([0-9a-f]+)(?:\s|$)', instructions[1])
    if branch is None or int(branch[1], 16) != destination:
        raise ValueError('initialization thunk does not forward to selected implementation')
    return 0


def inspect(elf):
    text = subprocess.check_output([str(LLVM / 'llvm-nm'), '-S', '--demangle', str(elf)], text=True)
    symbols = []
    for line in text.splitlines():
        match = re.match(r'([0-9a-f]{16}) ([0-9a-f]{16}) [tT] (.+)', line)
        if match:
            symbols.append((int(match[1], 16), int(match[2], 16), match[3]))

    def pick(predicate, optional=False, parse_frame=frame_from_disassembly):
        found = [s for s in symbols if predicate(s[2])]
        if not found and optional:
            return None
        if len(found) != 1:
            raise ValueError(f'expected one selected symbol, found {len(found)}')
        address, size, name = found[0]
        dis = subprocess.check_output([str(LLVM / 'llvm-objdump'), '-d', '--no-show-raw-insn',
            f'--start-address={address}', f'--stop-address={address+size}', str(elf)], text=True)
        return {'symbol': name, 'address': hex(address), 'frame': parse_frame(dis), 'disassembly': dis}

    init_name, thunk_name = choose_init_symbol([name for _, _, name in symbols])
    frames = {
        'entry': pick(lambda n: 'entry::run_main::' in n and 'closure#0' in n),
        'init': pick(lambda n: n == init_name),
        'start': pick(lambda n: n.endswith('pixel_mpss_runtime::start')),
        'opening': pick(lambda n: 'RuntimeDomain>::step_opening' in n),
        'transfer': pick(lambda n: 'sunfish_linux_rmtfs::Resources<' in n and '>::run::' in n and '>::transfer::' in n),
        'take': pick(lambda n: n.endswith('pixel_mpss_auth::take_runtime')),
        'start_live': pick(lambda n: 'sunfish_mpss_linux_transport_native::start_live::' in n),
    }
    # A separately outlined call_once wrapper is accepted only as an exact
    # zero-frame tail forwarder; an inlined init uses its full measured frame.
    init_entry = frames['init']
    if thunk_name is not None:
        frames['init_tail_thunk'] = pick(lambda n: n == thunk_name,
            parse_frame=lambda dis: zero_frame_init_thunk(dis, int(frames['init']['address'], 16)))
        init_entry = frames['init_tail_thunk']
    if not re.search(r'\bbl\s+' + re.escape(init_entry['address']) + r'(?:\s|$)', frames['entry']['disassembly']):
        raise ValueError('selected persistent entry does not call initialization implementation/thunk')
    frames['construct'] = pick(lambda n: n.endswith('pixel_mpss_runtime::construct_runtime'), True)
    frames['begin'] = pick(lambda n: n.endswith('pixel_mpss_runtime::begin_prepared_runtime'), True)
    split = frames['construct'] is not None and frames['begin'] is not None
    if (frames['construct'] is None) != (frames['begin'] is None):
        raise ValueError('partial handoff split')
    get = lambda key: frames[key]['frame']
    outer = get('entry') + get('init') + get('start')
    if split:
        construct = outer + get('construct') + max(get('take'), get('start_live'))
        begin = outer + get('begin') + get('opening') + get('transfer')
    else:
        construct = outer + max(get('take'), get('start_live'))
        begin = outer + get('opening') + get('transfer')
    return {'frames': frames, 'split': split, 'known_construction_bytes': construct,
            'known_begin_transfer_bytes': begin, 'known_peak_bytes': max(construct, begin),
            'scope': 'Selected nested frame regression budget; excludes TLS, indirect calls and other descendants; not whole-program WCS.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--elf', type=Path, required=True)
    parser.add_argument('--stack', type=lambda n: int(n, 0), default=0x500000)
    parser.add_argument('--headroom', type=lambda n: int(n, 0), default=0x40000)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = inspect(args.elf)
    result['input_elf_sha256'] = hashlib.sha256(args.elf.read_bytes()).hexdigest()
    result.update(mapped_stack_bytes=args.stack, required_headroom=args.headroom,
                  remaining=args.stack-result['known_peak_bytes'])
    result['passes'] = result['remaining'] >= args.headroom
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'frames'}, indent=2))
    if not result['passes']:
        raise SystemExit('Known nested handoff frames exceed the stack/headroom budget')


if __name__ == '__main__':
    main()
