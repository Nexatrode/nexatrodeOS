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


def checkpoint_framebuffer_bounds():
    """Return the exact two-buffer checkpoint range declared by its source header."""
    header = (HERE / 'loader-boot-checkpoints.h').read_text()
    address = re.search(
        r'^#define CHECKPOINT_FB \(\(volatile uint32_t \*\)\(uintptr_t\)0x([0-9a-fA-F]+)\)$',
        header, re.MULTILINE)
    dimensions = set(re.findall(
        r'const unsigned width = ([0-9]+), height = ([0-9]+)', header))
    require(address is not None, 'checkpoint framebuffer has a literal physical base')
    require(dimensions == {('1080', '2340')},
            'checkpoint framebuffer dimensions match both bounded draw helpers')
    for loop, count in (
            ('for (unsigned buffer = 0; buffer < 2; buffer++)', 3),
            ('for (unsigned y = 160; y < 320; y++)', 1),
            ('for (unsigned y = 160; y < 440; y++)', 1),
            ('for (unsigned column = 0; column < 24 && text[column]; column++)', 2),
            ('for (unsigned gy = 0; gy < 7; gy++)', 2),
            ('for (unsigned gx = 0; gx < 5; gx++)', 2),
            ('for (unsigned dy = 0; dy < scale; dy++)', 2),
            ('for (unsigned dx = 0; dx < scale; dx++)', 2)):
        require(header.count(loop) == count,
                'checkpoint framebuffer write bounds retain: ' + loop)
    require(header.count('unsigned x = 48 + column * 6 * scale;') == 2,
            'checkpoint framebuffer glyph columns remain bounded')
    require('180 + row * 120' in header and '230 + row * 120' in header,
            'checkpoint value rows remain inside the display')
    require(header.count('fb[') == 1,
            'AArch64 framebuffer writes use only the fixed-base store helper')
    require('"add %0, %1, %2, lsl #2\\n\\t"' in header
            and '"str %w3, [%0]"' in header,
            'AArch64 framebuffer store computes each address from its fixed base')
    compact_header = re.sub(r'\s+', '', header)
    for expression, count in (
            ('unsigned base = buffer * width * height;', 3),
            ('loader_checkpoint_store(fb, (uint64_t)base + (uint64_t)y * width + x,', 2),
            ('(uint64_t)base + (uint64_t)(200 + gy * scale + dy) * width + x + gx * scale + dx', 1),
            ('(uint64_t)base + (uint64_t)(y + gy * scale + dy) * width + x + gx * scale + dx', 1)):
        require(compact_header.count(re.sub(r'\s+', '', expression)) == count,
                'checkpoint framebuffer writes retain bounded indexing: ' + expression)
    start = int(address.group(1), 16)
    size = 2 * 1080 * 2340 * 4
    require(start + size <= 1 << 32,
            'checkpoint framebuffer range does not wrap its 32-bit address')
    return start, start + size


def parse_disassembly(text):
    """Return function-symbol addresses and decoded AArch64 instruction rows."""
    symbols = {}
    instructions = {}
    function = None
    for line in text.splitlines():
        symbol = re.match(r'^([0-9a-f]+) <([^>]+)>:$', line)
        if symbol:
            address, function = int(symbol.group(1), 16), symbol.group(2)
            symbols[function] = address
            continue
        row = re.match(r'^([0-9a-f]+):\s+[0-9a-f]{8}\s+([^\s]+)(?:\s+(.*))?$', line)
        if row and function:
            address, mnemonic = int(row.group(1), 16), row.group(2)
            operands = (row.group(3) or '').split('//', 1)[0].rstrip()
            targets = list(re.finditer(r'0x([0-9a-f]+)(?:\s+<([^>]+)>)?', operands))
            target = targets[-1] if targets else None
            target_address = int(target.group(1), 16) if target else None
            target_name = target.group(2).split('+', 1)[0] if target and target.group(2) else None
            instructions[address] = (mnemonic, operands, target_address, target_name, function)
    return symbols, instructions


def audit_closed_display_helpers(symbols, instructions, stack_range):
    """Prove display helpers stay in their closed call graph and return through LR."""
    allowed = {'loader_checkpoint', 'loader_checkpoint_hex', 'loader_checkpoint_draw_line'}
    framebuffer_start, framebuffer_end = checkpoint_framebuffer_bounds()
    require(stack_range is not None and stack_range[0] < stack_range[1],
            'loader stack range is bounded')
    require(framebuffer_end <= stack_range[0] or stack_range[1] <= framebuffer_start,
            'checkpoint framebuffer is disjoint from the runtime loader stack')
    absolute = ('absolute',)
    framebuffer_oob = ('framebuffer-out-of-range',)
    framebuffer_frame_size = 1080 * 2340 * 4
    # The checked source loops write only rows 0..439 in each of two buffers.
    framebuffer_write_end = framebuffer_start + framebuffer_frame_size + 440 * 1080 * 4
    require(framebuffer_write_end <= framebuffer_end,
            'bounded checkpoint writes fit inside the declared two-buffer range')

    def framebuffer_ref(address):
        return ('framebuffer', address)

    def is_framebuffer_ref(value):
        return isinstance(value, tuple) and value[:1] == ('framebuffer',)

    def is_dynamic_framebuffer_ref(value):
        return isinstance(value, tuple) and value[:1] == ('framebuffer-dynamic',)

    def classify_constant(value, width=64, signed_small=False):
        mask = (1 << width) - 1
        value &= mask
        if framebuffer_start <= value < framebuffer_end:
            return framebuffer_ref(value)
        maximum_positive_index = max(0, stack_range[0] - framebuffer_end)
        maximum_negative_index = max(0, framebuffer_start - stack_range[1])
        if signed_small and value >= (1 << (width - 1)):
            signed_value = value - (1 << width)
            if -maximum_negative_index <= signed_value <= maximum_positive_index:
                return ('scalar', signed_value)
        if value <= maximum_positive_index:
            return ('scalar', value)
        return absolute

    def immediate_value(token):
        match = re.fullmatch(r'#(-?(?:0x[0-9a-fA-F]+|[0-9]+))', token.strip())
        return int(match.group(1), 0) if match else None

    def shifted_immediate(parts):
        value = immediate_value(parts[1]) if len(parts) > 1 else None
        if value is None:
            return None
        if len(parts) > 2:
            shift = re.fullmatch(r'lsl\s+#(0x[0-9a-fA-F]+|[0-9]+)', parts[2])
            if shift:
                value <<= int(shift.group(1), 0)
        return value

    def scalar_value(value):
        return value[1] if isinstance(value, tuple) and value[:1] == ('scalar',) else None

    def add_alias(base, offset, operation, width=64, in_cycle=False, dynamic_framebuffer=None):
        offset_scalar = scalar_value(offset)
        if isinstance(base, int) and offset_scalar is not None:
            return base + (offset_scalar if operation == 'add' else -offset_scalar)
        if is_framebuffer_ref(base):
            if offset_scalar is None:
                return dynamic_framebuffer
            address = base[1] + (offset_scalar if operation == 'add' else -offset_scalar)
            if not framebuffer_start <= address < framebuffer_end:
                return framebuffer_oob
            if in_cycle and address != base[1]:
                return dynamic_framebuffer
            return framebuffer_ref(address)
        if is_dynamic_framebuffer_ref(base):
            if in_cycle:
                # Dynamic indexed writes are allowed only from a fixed base.
                # A loop-carried dynamic pointer cannot be widened without a
                # machine-level induction proof, so reject any nonzero/unknown
                # recurrence instead of assuming the source loop bounds apply.
                return dynamic_framebuffer if offset_scalar == 0 else framebuffer_oob
            if offset_scalar is None:
                return framebuffer_oob
            delta = offset_scalar if operation == 'add' else -offset_scalar
            low, high = base[1] + delta, base[2] + delta
            if low < framebuffer_start or high >= framebuffer_write_end:
                return framebuffer_oob
            return ('framebuffer-dynamic', low, high)
        if isinstance(base, tuple) and base[:1] == ('scalar',) and \
                isinstance(offset, tuple) and offset[:1] == ('scalar',):
            # Widen loop-carried scalars so CFG exploration has a finite state space.
            return ('scalar', None)
        if base == absolute:
            return absolute
        return None

    def reg_index(register):
        match = re.fullmatch(r'[xw]([0-9]+)', register)
        return int(match.group(1)) if match else None

    def alias_of(register, aliases, sp_delta):
        if register == 'sp':
            return sp_delta
        if register in ('xzr', 'wzr'):
            return ('scalar', 0)
        index = reg_index(register)
        if index is None:
            return None
        value = aliases[index]
        if register.startswith('w') and isinstance(value, int):
            return None
        if register.startswith('w') and isinstance(value, tuple) and value[:1] == ('scalar',):
            scalar = value[1]
            return ('scalar', None if scalar is None else scalar & 0xffffffff)
        if register.startswith('w') and is_framebuffer_ref(value) and value[1] >= 1 << 32:
            return None
        return value

    def parse_register_operand(token):
        match = re.match(r'^([xw][0-9]+|[xw]zr)(?:\s*,\s*(.*))?$', token)
        if not match:
            return None, None
        register, modifier = match.groups()
        return register, modifier

    def arithmetic_alias(mnemonic, operands, aliases, sp_delta, in_cycle, dynamic_framebuffer):
        parts = [part.strip() for part in operands.split(',')]
        if len(parts) < 3:
            return None
        dest, base = parts[0], parts[1]
        dest_index = reg_index(dest)
        if dest_index is None:
            return None
        base_value = alias_of(base, aliases, sp_delta)
        if base == 'sp' and mnemonic in ('add', 'sub'):
            immediate = immediate_value(parts[2])
            if immediate is not None:
                return sp_delta + (immediate if mnemonic == 'add' else -immediate)
        immediate = immediate_value(parts[2])
        width = 32 if dest.startswith('w') else 64
        if mnemonic in ('add', 'sub') and immediate is not None:
            offset = ('scalar', immediate)
            result = add_alias(base_value, offset, mnemonic, width, in_cycle, dynamic_framebuffer)
        elif mnemonic in ('add', 'sub'):
            operand, modifier = parse_register_operand(parts[2])
            if len(parts) > 3:
                modifier = ', '.join(parts[3:])
            other_value = alias_of(operand, aliases, sp_delta) if operand else None
            if modifier:
                shift = re.fullmatch(
                    r'(lsl|uxtw|sxtw|uxtx|sxtx)\s+#(0x[0-9a-fA-F]+|[0-9]+)', modifier)
                if not shift:
                    return None
                if not isinstance(other_value, tuple) or other_value[:1] != ('scalar',):
                    return add_alias(base_value, None, mnemonic, width,
                                     in_cycle, dynamic_framebuffer) \
                        if is_framebuffer_ref(base_value) else None
                scalar = other_value[1]
                amount = int(shift.group(2), 0)
                if scalar is not None:
                    kind = shift.group(1)
                    if kind == 'uxtw':
                        scalar &= 0xffffffff
                    elif kind == 'sxtw':
                        scalar &= 0xffffffff
                        if scalar & 0x80000000:
                            scalar -= 1 << 32
                    if kind in ('lsl', 'uxtw', 'sxtw'):
                        scalar <<= amount
                other_value = ('scalar', scalar)
            result = add_alias(base_value, other_value, mnemonic, width,
                               in_cycle, dynamic_framebuffer)
        else:
            result = None
        if dest.startswith('w') and isinstance(result, tuple) and result[:1] == ('scalar',):
            scalar = result[1]
            result = ('scalar', None if scalar is None else scalar & 0xffffffff)
        return result

    def instruction_alias(mnemonic, operands, target, aliases, sp_delta,
                          in_cycle, dynamic_framebuffer):
        parts = [part.strip() for part in operands.split(',')]
        dest = parts[0] if parts else ''
        dest_index = reg_index(dest)
        if dest_index is None:
            return None
        width = 32 if dest.startswith('w') else 64
        if mnemonic == 'mov':
            if len(parts) < 2:
                return None
            immediate = immediate_value(parts[1])
            if immediate is not None:
                return classify_constant(immediate, width, signed_small=True)
            return alias_of(parts[1], aliases, sp_delta)
        if mnemonic in ('movz', 'movn'):
            value = shifted_immediate(parts)
            if value is None:
                return None
            if mnemonic == 'movn':
                value = ~value
            return classify_constant(value, width, signed_small=True)
        if mnemonic == 'movk':
            old = alias_of(dest, aliases, sp_delta)
            old_value = scalar_value(old)
            if old_value is None and is_framebuffer_ref(old):
                old_value = old[1]
            if old_value is None:
                return None
            immediate = immediate_value(parts[1]) if len(parts) > 1 else None
            if immediate is None:
                return None
            shift_match = re.fullmatch(r'lsl\s+#(0x[0-9a-fA-F]+|[0-9]+)', parts[2]) if len(parts) > 2 else None
            shift = int(shift_match.group(1), 0) if shift_match else 0
            mask = 0xffff << shift
            value = (old_value & ~mask) | ((immediate & 0xffff) << shift)
            return classify_constant(value, width)
        if mnemonic in ('adr', 'adrp'):
            return classify_constant(target, 64) if target is not None else absolute
        if mnemonic in ('add', 'sub'):
            return arithmetic_alias(mnemonic, operands, aliases, sp_delta,
                                    in_cycle, dynamic_framebuffer)
        if mnemonic in ('madd', 'mul'):
            sources = parts[1:]
            if all(scalar_value(alias_of(source.split()[0], aliases, sp_delta)) is not None
                   or (isinstance(alias_of(source.split()[0], aliases, sp_delta), tuple)
                       and alias_of(source.split()[0], aliases, sp_delta)[:1] == ('scalar',))
                   for source in sources):
                return ('scalar', None)
        return None

    def register_width(register):
        if register in ('xzr', 'x30') or register.startswith('x'):
            return 8
        if register == 'wzr' or register.startswith('w') or register.startswith('s'):
            return 4
        if register.startswith('d'):
            return 8
        if register.startswith('q'):
            return 16
        if register.startswith('h'):
            return 2
        if register.startswith('b'):
            return 1
        return None

    def store_width(mnemonic, operands):
        if mnemonic in ('stg', 'stzg'):
            return 16
        if mnemonic in ('st2g', 'stz2g'):
            return 32
        before_memory = operands.split('[', 1)[0]
        registers = re.findall(r'\b(?:[wx][0-9]+|[wx]zr|[sdqbh][0-9]+)\b', before_memory)
        pair = mnemonic in ('stp', 'stnp', 'stxp', 'stlxp', 'stgp') or mnemonic.startswith('casp')
        if pair:
            selected = registers[-2:]
            if len(selected) != 2:
                return None
            widths = [register_width(register) for register in selected]
            return sum(widths) if all(widths) else None
        if mnemonic.startswith(('cas', 'swp', 'ldadd', 'ldclr', 'ldset', 'ldeor',
                                'ldsmax', 'ldsmin', 'ldumax', 'ldumin')):
            widths = [register_width(register) for register in registers[-2:]]
            return max(widths) if widths and all(widths) else None
        if mnemonic in ('strb', 'sturb', 'stlrb', 'stllrb', 'sttrb', 'stxrb', 'stlxrb',
                        'strh', 'sturh', 'stlrh', 'stllrh', 'sttrh', 'stxrh', 'stlxrh',
                        'str', 'stur', 'stlr', 'stllr', 'sttr', 'stxr', 'stlxr'):
            if mnemonic.endswith('b'):
                return 1
            if mnemonic.endswith('h'):
                return 2
            register = registers[-1] if registers else None
            return register_width(register) if register else None
        width = register_width(registers[-1]) if registers else None
        return width if mnemonic in ('str', 'stur', 'stlr', 'stllr', 'sttr') else None

    write_rmw = ('cas', 'swp', 'ldadd', 'ldclr', 'ldset', 'ldeor',
                 'ldsmax', 'ldsmin', 'ldumax', 'ldumin')
    ordered_symbols = sorted((address, name) for name, address in symbols.items())
    for name in allowed:
        require(name in symbols, 'compiled framebuffer helper: ' + name)
        start = symbols[name]
        end = next((address for address, _ in ordered_symbols if address > start), None)
        require(end is not None, 'bounded framebuffer helper: ' + name)
        body = {address: row for address, row in instructions.items() if start <= address < end}
        require(start in body, 'framebuffer helper entry instruction: ' + name)
        cyclic_addresses = set()
        max_framebuffer_store_offset = 0
        for pc, (op, args, branch_target, _, _) in body.items():
            if (op == 'b' or op.startswith('b.') or op in ('cbz', 'cbnz', 'tbz', 'tbnz')) \
                    and branch_target is not None and branch_target < pc:
                cyclic_addresses.update(candidate for candidate in body
                                        if branch_target <= candidate <= pc)
            if op.startswith('st') or op.startswith(write_rmw):
                access = re.search(r'\[(sp|x[0-9]+)(?:,\s*([^,\]]+))?\]', args)
                if access and access.group(1) != 'sp':
                    token = access.group(2)
                    if token and token.startswith('#'):
                        amount = int(token[1:], 0)
                        max_framebuffer_store_offset = max(max_framebuffer_store_offset, amount)
        dynamic_high = framebuffer_write_end - max_framebuffer_store_offset - 4
        require(framebuffer_start <= dynamic_high < framebuffer_write_end,
                'dynamic framebuffer pointer interval is bounded for every emitted store')
        dynamic_framebuffer = ('framebuffer-dynamic', framebuffer_start, dynamic_high)
        entry_aliases = [None] * 31
        if name == 'loader_checkpoint_draw_line':
            # Its only closed callers pass a buffer index in w0 and text row in w2.
            entry_aliases[0] = ('scalar', None)
            entry_aliases[2] = ('scalar', None)
        pending = [(start, 0, None, False, tuple(entry_aliases))]
        # State: address, SP delta, saved LR pair base, LR dirty, register stack aliases.
        visited = set()
        returns = 0
        while pending:
            address, sp_delta, saved_lr_slot, lr_dirty, aliases = pending.pop()
            state = (address, sp_delta, saved_lr_slot, lr_dirty, aliases)
            if state in visited:
                continue
            visited.add(state)
            require(address in body, 'framebuffer helper path stays in code: ' + name + ' at ' + hex(address))
            mnemonic, operands, target, target_name, _ = body[address]
            link_pair = re.search(r'\bx29\s*,\s*x30\b', operands) is not None

            immediate = r'(-?(?:0x[0-9a-fA-F]+|[0-9]+))'
            stack_arithmetic = (re.match(
                r'^sp,\s*sp,\s*#' + immediate + r'(?:\s*,.*)?$', operands)
                if mnemonic in ('add', 'sub') else None)
            memory = re.search(
                r'\[(sp|x[0-9]+)(?:,\s*([^,\]]+))?\](!)?(?:,\s*(#[^,\s]+))?', operands)
            next_sp_delta = sp_delta
            memory_address = None
            memory_base = None
            memory_base_alias = None
            memory_offset = None
            memory_post = None
            if stack_arithmetic:
                amount = int(stack_arithmetic.group(1), 0)
                next_sp_delta += amount if mnemonic == 'add' else -amount
            elif memory:
                memory_base = memory.group(1)
                offset_token = memory.group(2)
                pre_index = bool(memory.group(3))
                memory_post = memory.group(4)
                memory_offset = int(offset_token[1:], 0) if offset_token and offset_token.startswith('#') else (0 if not offset_token else None)
                if memory_base == 'sp':
                    if memory_offset is not None:
                        memory_address = sp_delta + memory_offset
                        if pre_index:
                            next_sp_delta = memory_address
                    if memory_post:
                        next_sp_delta = sp_delta + int(memory_post[1:], 0)
                else:
                    base_value = alias_of(memory_base, aliases, sp_delta)
                    memory_base_alias = base_value
                    if isinstance(base_value, int) and memory_offset is not None:
                        memory_address = base_value + memory_offset
                        base_index = reg_index(memory_base)
                        if pre_index and base_index is not None:
                            aliases = tuple(memory_address if index == base_index else value
                                            for index, value in enumerate(aliases))
                        if memory_post and base_index is not None:
                            post_value = base_value + int(memory_post[1:], 0)
                            aliases = tuple(post_value if index == base_index else value
                                            for index, value in enumerate(aliases))
                    elif is_framebuffer_ref(base_value) and memory_offset is not None:
                        memory_address = add_alias(
                            base_value, ('scalar', memory_offset), 'add',
                            in_cycle=False, dynamic_framebuffer=dynamic_framebuffer)
                        base_index = reg_index(memory_base)
                        if pre_index and base_index is not None:
                            updated = add_alias(
                                base_value, ('scalar', memory_offset), 'add',
                                in_cycle=address in cyclic_addresses,
                                dynamic_framebuffer=dynamic_framebuffer)
                            aliases = tuple(updated if index == base_index else value
                                            for index, value in enumerate(aliases))
                        if memory_post and base_index is not None:
                            updated = add_alias(
                                base_value, ('scalar', int(memory_post[1:], 0)), 'add',
                                in_cycle=address in cyclic_addresses,
                                dynamic_framebuffer=dynamic_framebuffer)
                            aliases = tuple(updated if index == base_index else value
                                            for index, value in enumerate(aliases))
                    elif is_dynamic_framebuffer_ref(base_value) and memory_offset is not None:
                        memory_address = add_alias(
                            base_value, ('scalar', memory_offset), 'add',
                            in_cycle=False, dynamic_framebuffer=dynamic_framebuffer)
                    elif base_value == framebuffer_oob:
                        memory_address = framebuffer_oob
            elif re.search(r'\[sp\b', operands):
                raise ValueError('unrecognized framebuffer helper SP memory operand: '
                                 + name + ' at ' + hex(address))
            elif re.match(r'^sp\s*,', operands):
                raise ValueError('unrecognized framebuffer helper SP update: ' + name + ' at ' + hex(address))

            if mnemonic == 'ret':
                require(operands.strip() in ('', 'x30'),
                        'framebuffer helper uses normal LR return: ' + name + ' at ' + hex(address))
                require(not lr_dirty,
                        'framebuffer helper restores LR after calls: ' + name + ' at ' + hex(address))
                require(saved_lr_slot is None and sp_delta == 0,
                        'framebuffer helper restores its exact LR slot and stack depth: '
                        + name + ' at ' + hex(address))
                returns += 1
                continue
            if mnemonic in ('br', 'blr', 'eret', 'drps', 'brk', 'hlt', 'hvc', 'smc', 'udf'):
                raise ValueError('non-returning framebuffer helper transfer: ' + name + ' at ' + hex(address))

            store_like = mnemonic.startswith('st') or mnemonic.startswith(write_rmw)
            if store_like and saved_lr_slot is not None:
                require(memory is not None,
                        'framebuffer helper write has a decoded memory operand: ' + name + ' at ' + hex(address))
                if memory_base == 'sp':
                    require(memory_address is not None,
                            'framebuffer helper SP write has a bounded offset: ' + name + ' at ' + hex(address))
                elif is_framebuffer_ref(memory_base_alias) or is_dynamic_framebuffer_ref(memory_base_alias):
                    require(memory_address is not None,
                            'framebuffer helper framebuffer write has a bounded address: '
                            + name + ' at ' + hex(address))
                elif isinstance(memory_base_alias, int):
                    require(memory_address is not None,
                            'framebuffer helper stack-alias write has a bounded offset: '
                            + name + ' at ' + hex(address))
                else:
                    require(memory_base is not None,
                            'framebuffer helper write base is classified: ' + name + ' at ' + hex(address))
                    raise ValueError('framebuffer helper write base may alias saved LR: '
                                     + name + ' at ' + hex(address))
                width = store_width(mnemonic, operands)
                require(width is not None,
                        'framebuffer helper write width is bounded: ' + name + ' at ' + hex(address))
                if isinstance(memory_address, int):
                    saved_lr_start, saved_lr_end = saved_lr_slot + 8, saved_lr_slot + 16
                    write_end = memory_address + width
                    require(write_end <= saved_lr_start or memory_address >= saved_lr_end,
                            'framebuffer helper store does not overwrite saved LR: '
                            + name + ' at ' + hex(address))
                elif is_framebuffer_ref(memory_address):
                    require(framebuffer_start <= memory_address[1]
                            and memory_address[1] + width <= framebuffer_end,
                            'framebuffer store address is outside the declared interval: '
                            + name + ' at ' + hex(address))
                elif is_dynamic_framebuffer_ref(memory_address):
                    require(width == 4 and framebuffer_start <= memory_address[1]
                            and memory_address[2] + width <= framebuffer_write_end,
                            'dynamic framebuffer store escapes its source-bounded write interval: '
                            + name + ' at ' + hex(address))
                else:
                    raise ValueError('framebuffer helper store address is not bounded: '
                                     + name + ' at ' + hex(address))

            next_saved_slot, next_dirty = saved_lr_slot, lr_dirty
            if mnemonic == 'stp' and link_pair:
                require(memory_address is not None,
                        'framebuffer helper saves LR in an SP-relative slot: ' + name + ' at ' + hex(address))
                require(not lr_dirty,
                        'framebuffer helper saves original LR before nested calls: ' + name)
                require(saved_lr_slot is None,
                        'framebuffer helper has one live LR save slot: ' + name + ' at ' + hex(address))
                next_saved_slot = memory_address
            elif mnemonic == 'ldp' and link_pair:
                require(memory_address is not None and saved_lr_slot == memory_address,
                        'framebuffer helper restores LR from its matching stack slot: '
                        + name + ' at ' + hex(address))
                next_saved_slot = None
                next_dirty = False
            elif re.search(r'\bx30\b', operands):
                require(saved_lr_slot is not None,
                        'framebuffer helper preserves LR before reuse: ' + name + ' at ' + hex(address))
                next_dirty = True

            if mnemonic == 'bl':
                require(target_name in allowed and target == symbols.get(target_name),
                        'framebuffer helper call is closed: ' + name + ' at ' + hex(address))
                require(next_saved_slot is not None,
                        'framebuffer helper saves LR before nested call: ' + name + ' at ' + hex(address))
                next_dirty = True
                successors = [address + 4]
            elif mnemonic == 'b':
                require(target is not None and start <= target < end,
                        'framebuffer helper branch stays local: ' + name + ' at ' + hex(address))
                successors = [target]
            elif mnemonic.startswith('b.') or mnemonic in ('cbz', 'cbnz', 'tbz', 'tbnz'):
                require(target is not None and start <= target < end,
                        'framebuffer helper conditional branch stays local: ' + name + ' at ' + hex(address))
                successors = [target, address + 4]
            else:
                successors = [address + 4]

            next_aliases = list(aliases)
            if mnemonic == 'bl':
                for index in range(19):
                    next_aliases[index] = None
                next_aliases[30] = absolute
            elif mnemonic in ('mov', 'movz', 'movn', 'movk', 'adr', 'adrp', 'add', 'sub', 'madd', 'mul'):
                dest = operands.split(',', 1)[0].strip()
                dest_index = reg_index(dest)
                if dest_index is not None:
                    next_aliases[dest_index] = instruction_alias(
                        mnemonic, operands, target, aliases, sp_delta,
                        address in cyclic_addresses, dynamic_framebuffer)
            elif mnemonic == 'ldp':
                loaded = re.findall(r'\b(?:x[0-9]+|w[0-9]+)\b', operands.split('[', 1)[0])
                for register in loaded:
                    index = reg_index(register)
                    if index is not None:
                        next_aliases[index] = None
                if link_pair and saved_lr_slot is not None:
                    next_aliases[30] = absolute
            elif mnemonic.startswith('ld'):
                parts = [part.strip() for part in operands.split(',')]
                dest_index = reg_index(parts[0]) if parts else None
                if dest_index is not None:
                    next_aliases[dest_index] = None
            elif mnemonic not in ('ret', 'cmp', 'cmn', 'tst', 'tbz', 'tbnz', 'cbz', 'cbnz',
                                   'b', 'br', 'blr', 'eret', 'drps', 'dsb', 'dmb', 'isb', 'nop') \
                    and not mnemonic.startswith('b.') and not store_like:
                parts = [part.strip() for part in operands.split(',')]
                dest_index = reg_index(parts[0]) if parts else None
                if dest_index is not None:
                    next_aliases[dest_index] = None
            for successor in successors:
                require(successor in body,
                        'framebuffer helper has no fallthrough escape: ' + name + ' at ' + hex(address))
                pending.append((successor, next_sp_delta, next_saved_slot, next_dirty,
                                tuple(next_aliases)))
        require(returns > 0, 'reachable normal return from framebuffer helper: ' + name)
    return {'helpers_checked': len(allowed), 'reachable_helper_returns_use_lr': True,
            'helper_paths_stay_in_bounded_functions': True}


def audit_guard_control_flow(instructions, entry, relocation_call, stop, allowed_helpers,
                             continue_success=False):
    """Prove source-guard failures stop, and optionally the pass path relocates."""
    conditional = {'cbz', 'cbnz', 'tbz', 'tbnz'}
    state = {}
    def visit(address):
        if address == stop:
            state[address] = 'done'
            return
        if continue_success and address == relocation_call:
            mnemonic, _, target, target_name, _ = instructions[address]
            require(mnemonic == 'bl' and target_name == 'relocate',
                    'passing source guard reaches the unique relocation call')
            state[address] = 'relocated'
            return
        require(address < relocation_call, 'reachable source guard instruction reaches relocation')
        require(address in instructions, 'source guard control flow reaches non-code at ' + hex(address))
        if state.get(address) == 'visiting':
            raise ValueError('source guard contains a non-diagnostic control-flow cycle at ' + hex(address))
        if state.get(address) == 'done':
            return
        state[address] = 'visiting'
        mnemonic, _, target, target_name, _ = instructions[address]
        if mnemonic == 'bl':
            require(target_name in allowed_helpers,
                    'source guard call is not a closed display helper at ' + hex(address))
            successors = [address + 4]
        elif mnemonic == 'b':
            require(target is not None, 'unresolved source guard branch at ' + hex(address))
            require(target != address or address == stop, 'unexpected source guard self-loop')
            successors = [] if target == address else [target]
        elif mnemonic.startswith('b.') or mnemonic in conditional:
            require(target is not None, 'unresolved source guard conditional branch at ' + hex(address))
            successors = [target, address + 4]
        elif mnemonic in ('br', 'blr', 'ret', 'eret', 'drps'):
            raise ValueError('indirect source guard control transfer at ' + hex(address))
        else:
            successors = [address + 4]
        for successor in successors:
            visit(successor)
        state[address] = 'done'
    visit(entry)
    require(state.get(stop) == 'done', 'diagnostic stop loop is not reachable')
    if continue_success:
        require(state.get(relocation_call) == 'relocated',
                'source-guard success path reaches relocation')
    return {'entry_address': hex(entry), 'relocation_call_address': hex(relocation_call),
            'stop_loop_address': hex(stop), 'reachable_guard_instructions': len(state),
            'closed_display_helpers': sorted(allowed_helpers),
            'all_failure_paths_stop_before_relocation': True,
            'pass_path_reaches_relocation': continue_success}


def validate_source_diagnostic(profile, out, continue_success=False):
    """Audit labeled source-guard failures and the configured pass behavior."""
    code_path = out / 'loader-checkpoint-source/src/aarch64/crt0.S'
    elf_path = out / 'loader-relocated-build/loader.elf'
    code = code_path.read_text()
    require(re.search(
        r'adrp\s+x1,\s*_stack\s*\n\s*add\s+x1,\s*x1,\s*STACK_SIZE\s*\n\s*mov\s+sp,\s*x1',
        code) is not None,
        'diagnostic display helpers run on the compiled loader stack range')
    guard = code.split('    /* May not have been loaded in the right location.', 1)[0]
    for unchanged_limit in ('ldr x1, =0x20000', 'ldr x1, =0x6000000'):
        require(unchanged_limit in guard, 'source guard retains existing bound: ' + unchanged_limit)
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
    if profile.get('native_source_report_guard', False):
        require('ldr x1, =0x80081000' in guard
                and 'b.ne .Lnative_source_address_fail' in guard
                and 'add x4, x0, x3' in guard
                and 'ldr x1, =0x85700000' in guard
                and 'b.hi .Lnative_source_reservation_fail' in guard,
                'observed source has an explicit hypervisor-reservation ceiling')
        require(re.search(r'\.Lnative_source_reservation_fail:\n\s+mov x1, x4\n'
                          r'\s+adr x0, \.Lnative_source_reservation_text\n'
                          r'\s+bl loader_checkpoint_hex\n\s+b \.Lnative_source_diagnostic_stop', guard)
                is not None and '.asciz "SOURCE RANGE END"' in guard,
                'reservation failure flushes the measured occupied end then stops')
    else:
        require('0x80081000' not in guard and '0x85700000' not in guard,
                'legacy diagnostic mode retains its original source allowlist')
    failure_blocks = (
        ('.Lnative_source_address_fail:', '.Lnative_source_prefix_fail:', True),
        ('.Lnative_source_prefix_fail:', '.Lnative_source_overflow_fail:', True),
        ('.Lnative_source_overflow_fail:', '.Lnative_source_extent_fail:', True),
        ('.Lnative_source_extent_fail:', '.Lnative_source_diagnostic_stop:', False),
    )
    for label, next_label, has_explicit_stop in failure_blocks:
        block = guard.split(label, 1)[1].split(next_label, 1)[0]
        if has_explicit_stop:
            require('b .Lnative_source_diagnostic_stop' in block,
                    'failure handler stops after displaying ' + label)
        else:
            require('bl loader_checkpoint_hex' in block,
                    'extent failure displays its measured value before falling into the stop loop')
    if profile.get('native_source_report_guard', False):
        reservation_block = guard.split('.Lnative_source_reservation_fail:', 1)[1].split(
            '.Lnative_source_diagnostic_stop:', 1)[0]
        require('b .Lnative_source_diagnostic_stop' in reservation_block,
                'reservation failure cannot fall through to the pass path')
    stop = '.Lnative_source_diagnostic_stop:\n    b .Lnative_source_diagnostic_stop'
    expected_calls = 7 if profile.get('native_source_report_guard', False) else 6
    require(stop in guard and guard.count('bl loader_checkpoint_hex') == expected_calls,
            'labeled source guard emits each measured value')
    if continue_success:
        require('    b .Lnative_source_continue' in guard
                and '.Lnative_source_continue:' in guard
                and guard.index('    b .Lnative_source_continue') < guard.index('.Lnative_source_address_fail:'),
                'passing guard skips failure handlers and continues to relocation')
        require(profile.get('native_source_report_guard', False)
                and not profile.get('native_source_diagnostic', False),
                'pass-through source-guard profile is explicit')
    else:
        require('    b .Lnative_source_diagnostic_stop' in guard
                and not profile.get('native_source_report_guard', False),
                'standalone source diagnostic stops even when checks pass')
    require('IMAGE SOURCE STOP' not in guard, 'diagnostic never falls back to ambiguous stop')
    disassembly = subprocess.check_output(
        ['/opt/homebrew/opt/llvm/bin/llvm-objdump', '-d', str(elf_path)], text=True)
    symbols, instructions = parse_disassembly(disassembly)
    require('relocate' in symbols and '_text' in symbols, 'compiled loader entry and relocate symbols')
    relocation_calls = [address for address, row in instructions.items()
                        if row[0] == 'bl' and row[2] == symbols['relocate'] and row[3] == 'relocate']
    require(len(relocation_calls) == 1, 'unique compiled call to relocate')
    relocation_call = relocation_calls[0]
    machine_guard = {address: row for address, row in instructions.items() if address < relocation_call}
    if continue_success:
        machine_guard[relocation_call] = instructions[relocation_call]
    require(sum(row[0] == 'bl' and row[3] == 'loader_checkpoint_hex'
                for row in machine_guard.values()) == expected_calls,
            'all six source diagnostics compiled to the AArch64 image')
    self_loops = [address for address, row in machine_guard.items()
                  if row[0] == 'b' and row[2] == address]
    require(len(self_loops) == 1, 'unique compiled diagnostic stop loop before relocate')
    helpers = {'loader_checkpoint', 'loader_checkpoint_hex', 'loader_checkpoint_draw_line'}
    loader_header = (out / 'loader-checkpoint-source/src/loader.h').read_text()
    stack_size = re.search(r'^#define STACK_SIZE ([0-9]+)$', loader_header, re.MULTILINE)
    require(stack_size is not None, 'compiled loader declares its stack size')
    require(int(stack_size.group(1)) == 4096,
            'diagnostic loader uses the checked 4 KiB stack extent')
    nm_text = subprocess.check_output(
        ['/opt/homebrew/opt/llvm/bin/llvm-nm', '-n', str(elf_path)], text=True)
    stack_symbol = re.search(r'^([0-9a-f]+)\s+B\s+_stack$', nm_text, re.MULTILINE)
    require(stack_symbol is not None, 'compiled loader exports its stack base')
    stack_start = int(stack_symbol.group(1), 16)
    stack_range = (stack_start, stack_start + int(stack_size.group(1)))
    helper_audit = audit_closed_display_helpers(symbols, instructions, stack_range)
    cfg = audit_guard_control_flow(machine_guard, symbols['_text'], relocation_call,
                                   self_loops[0], helpers, continue_success=continue_success)
    return {'generated_assembly_sha256': hashlib.sha256(code_path.read_bytes()).hexdigest(),
            'compiled_loader_sha256': hashlib.sha256(elf_path.read_bytes()).hexdigest(),
            'loader_stack_range': [hex(stack_range[0]), hex(stack_range[1])],
            'diagnostic_calls': expected_calls, **helper_audit, **cfg}


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
        source_report_guard = profile.get('native_source_report_guard', False)
        require(profile.get('native_source_diagnostic', False) or source_report_guard,
                'source-guard diagnostic loader profile')
        require(profile['native_startup_entry_guard'] and profile['base_sdk_unchanged']
                and profile['kernel_initialiser_monitor_unchanged']
                and not profile['task_images'] and profile['entry_interrupt_masked'],
                'closed diagnostic loader profile')
        require(profile['loader_link_address'] == '0xc0000000'
                and profile['expected_runtime_source'] == '0xb1281000'
                and profile['source_image_base'] == '0xb1280000'
                and profile['native_startup_max_relocation_bytes'] == 0x6000000,
                'exact runtime source/envelope guard')
        result = validate_source_diagnostic(profile, out, continue_success=source_report_guard)
        (out / 'source-guard-loader-audit.json').write_text(json.dumps(result, indent=2) + '\n')
        if source_report_guard:
            print('PASS: generated AArch64 source guard reports labeled failures, stops on failure, and reaches relocation on pass.')
        else:
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
            and profile['expected_runtime_source'] == '0xb1281000'
            and profile['source_image_base'] == '0xb1280000'
            and profile['native_startup_max_relocation_bytes'] == 0x6000000, 'exact runtime source/envelope guard')
    code = (out / 'loader-checkpoint-source/src/aarch64/crt0.S').read_text()
    require(code.index('ldr x1, =0xb1281000') < code.index('    bl     relocate'), 'source guard before relocation')
    source_guard_audit = None
    if profile.get('native_source_diagnostic', False) or profile.get('native_source_report_guard', False):
        source_guard_audit = validate_source_diagnostic(
            profile, out, continue_success=profile.get('native_source_report_guard', False))
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
    source_range_audit = None
    if profile.get('native_source_report_guard', False):
        source_base, source_entry = 0x80080000, 0x80081000
        source_end = source_base + len(image)
        checked_dtbs = [ROOT / '.cache/pixel-sunfish/live-runtime.dtb',
                        ROOT / '.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb']
        hyp_starts = []
        for dt in checked_dtbs:
            cells = memory.read_cells(dt, '/memory', 'reg')
            require(cells and len(cells) % 4 == 0, 'RAM cells for observed Fastboot source audit')
            ram = [((cells[i] << 32) | cells[i+1], (cells[i+2] << 32) | cells[i+3])
                   for i in range(0, len(cells), 4)]
            reservations, _ = memory.read_regions(dt)
            hyp = [region for region in reservations if region[0].startswith('hyp_region@')]
            require(len(hyp) == 1 and hyp[0][1] == 0x85700000 and hyp[0][3],
                    'fixed no-map hypervisor reservation in ' + dt.name)
            hyp_starts.append(hyp[0][1])
            layout.validate_range('exact observed Fastboot source image', source_base,
                                  len(image), ram, reservations)
            layout.validate_range('relocated loader destination', 0xc0000000,
                                  0x6000000, ram, reservations)
            layout.validate_disjoint(source_base, len(image), 0xc0000000, 0x6000000)
        require(source_entry + len(payload) <= min(hyp_starts),
                'actual loader payload ends below the hypervisor reservation')
        require(source_entry + 0x6000000 > min(hyp_starts),
                '96 MiB source ceiling is rejected as a physical envelope at the measured base')
        source_range_audit = {
            'source_entry': hex(source_entry), 'source_base': hex(source_base),
            'exact_image_bytes': len(image), 'exact_image_end': hex(source_end),
            'loader_payload_end': hex(source_entry + len(payload)),
            'hyp_region_start': hex(min(hyp_starts)),
            '96mib_ceiling_crosses_reservation': True,
            'exact_source_and_relocation_ranges_safe_in_live_and_kernel_dtbs': True,
        }
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
        layout.validate_range('runtime-guarded source envelope', 0xb1280000, 0x6001000, ram, reserved)
        layout.validate_range('runtime-guarded relocated envelope', 0xc0000000, 0x6000000, ram, reserved)
        layout.validate_range('complete initial task', initial_start, initial_end - initial_start, ram, reserved)
        for start, length in targets:
            layout.validate_range('actual copy destination', start, length, ram, reserved)
    layout.validate_disjoint(0xb1280000, len(image), 0xc0000000, len(payload))
    for i, (start, length) in enumerate(targets):
        layout.validate_disjoint(start, length, 0xc0000000, len(payload))
        layout.validate_disjoint(start, length, 0xb1280000, len(image))
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
    result = dict(source_image_base='0xb1280000', source_placement_checked_before_relocation=True,
                  unexpected_source_stops_before_copy=True, relocated_loader_base='0xc0000000',
                  loader_bytes=len(payload), initial_task_start=hex(initial_start), initial_task_end=hex(initial_end),
                  copy_targets=[[hex(a), hex(b)] for a, b in targets],
                  reclaimed_kernel_boot_range=[hex(v) for v in reclaimed],
                  reclaimed_kernel_boot_frames=reused_boot_frames,
                  full_envelopes_and_copy_targets_avoid_three_dt_reservations=True,
                  base_kernel_monitor_and_initialiser_unchanged=True,
                  scope='Offline range/packing audit plus compiled runtime source guard. Device boot and actual MPSS authentication remain unproved.')
    result['source_guard_diagnostic_audit'] = source_guard_audit
    result['observed_fastboot_source_range_audit'] = source_range_audit
    for path, expected in hashes.items():
        require(hashlib.sha256(Path(path).read_bytes()).hexdigest() == expected, 'audit input changed')
    result['input_sha256'] = hashes
    (out / 'boot-layout-audit.json').write_text(json.dumps(result, indent=2) + '\n')
    print('PASS: complete large-image source, relocation, initial task, copy targets and preserved boot components.')


if __name__ == '__main__':
    main()
