#!/usr/bin/env python3
"""Bind rebuilt SDK code and serialized NC map flags to the packed loader bytes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def text_section(path):
    data = path.read_bytes()
    assert data[:4] == b'\x7fELF' and struct.unpack_from('<H', data, 18)[0] == 183
    table = struct.unpack_from('<Q', data, 40)[0]
    size, count, index = struct.unpack_from('<HHH', data, 58)
    sections = [struct.unpack_from('<IIQQQQIIQQ', data, table + i * size) for i in range(count)]
    strings = sections[index][4]
    result = [data[s[4]:s[4]+s[5]] for s in sections if data[strings+s[0]:].startswith(b'.text\0')]
    assert len(result) == 1 and result[0]
    return result[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('profile', type=Path)
    args = parser.parse_args()
    out = args.profile.resolve()
    subprocess.run(['python3', str(ROOT / 'platforms/sunfish/build-table-normal-nc-sdk.py'),
                    '--prepare-only'], check=True)
    fresh_sdk = out / 'microkit-source/release/microkit-sdk-2.3.1-sunfish-table-normal-nc'
    for name in ['sel4', 'initialiser', 'monitor']:
        relative = f'board/sunfish/debug/elf/{name}.elf'
        assert (out / 'sdk' / relative).read_bytes() == (fresh_sdk / relative).read_bytes(), name
    tool = out / 'sdk/bin/microkit'
    assert tool.read_bytes() == (out / 'microkit-source/target/release/microkit').read_bytes()
    loader = out / 'sdk/board/sunfish/debug/elf/loader.elf'
    assert hashlib.sha256(loader.read_bytes()).hexdigest() == 'f1878fe5aaafb0073d4cc192c73c801dac9010ac1ed695a529da315653534fec'
    # Re-run the pinned tool/serializer against the audited inputs and rebuilt SDK.
    # Exact whole-file equality covers all loaded code/data, enumerated tool
    # injections and all archived capabilities, not only text/selected NC fields.
    with tempfile.TemporaryDirectory(prefix='sunfish-nc-repack-') as temp:
        temp = Path(temp)
        with (out / 'repack-verification.log').open('w') as log:
            subprocess.run([str(tool), str(out / 'native-startup-mpss-runtime-5m.system'),
                            '--search-path', str(out), '--board', 'sunfish', '--config', 'debug',
                            '-o', str(temp / 'loader.img'), '-r', str(temp / 'report.txt'),
                            '--capdl-json', str(temp / 'capdl.json')], check=True,
                           stdout=log, stderr=subprocess.STDOUT)
        for name in ['loader.img', 'report.txt', 'capdl.json']:
            assert (temp / name).read_bytes() == (out / name).read_bytes(), f'non-reproducible {name}'
    payload = (out / 'loader.img').read_bytes()
    hashes = {}
    for name in ['sel4', 'initialiser', 'monitor']:
        elf = out / f'sdk/board/sunfish/debug/elf/{name}.elf'
        assert text_section(elf) in payload, f'{name} text missing from packed loader'
        hashes[name] = hashlib.sha256(elf.read_bytes()).hexdigest()
    # The custom type is the initializer's actual serialized CapDL PHDR.
    marker = struct.pack('<II', 0x64c34003, 4)
    assert payload.count(marker) == 1
    phdr = struct.unpack_from('<IIQQQQQQ', payload, payload.index(marker))
    header_at = payload.index(struct.pack('<Q', 0x5e14dead14de5ead))
    _, total, _, initial_start, initial_end, pv_offset, _, count = struct.unpack_from('<8Q', payload, header_at)
    assert total + header_at == len(payload) and 0 < count <= 64
    physical = phdr[3] + pv_offset
    assert initial_start <= physical < physical + phdr[5] <= initial_end
    data_start = header_at + 64 + count * 32
    matches = []
    for i in range(count):
        start, length, offset, kind = struct.unpack_from('<4Q', payload, header_at + 64 + i * 32)
        if start <= physical and physical + phdr[5] <= start + length:
            assert kind == 1
            at = data_start + offset + physical - start
            matches.append(payload[at:at+phdr[5]])
    assert len(matches) == 1 and len(matches[0]) == phdr[5]
    work = out / 'packed-archive-audit'
    (work / 'src').mkdir(parents=True, exist_ok=True)
    (work / 'packed-spec.bin').write_bytes(matches[0])
    (work / 'src/main.rs').write_bytes((ROOT / 'tests/sunfish_table_normal_nc_archive.rs').read_bytes())
    types = out / 'rust-source/crates/sel4-capdl-initializer/types'
    (work / 'Cargo.toml').write_text(
        '[package]\nname="sunfish-packed-nc-audit"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n'
        + 'sel4-capdl-initializer-types={path=' + json.dumps(str(types)) + '}\n'
        + 'rkyv={version="0.8.17",default-features=false,features=["alloc","bytecheck","pointer_width_32"]}\n')
    # Same exact rkyv dependency versions as the tool that serialized these bytes.
    lock = out / 'microkit-source/Cargo.lock'
    (work / 'Cargo.lock').write_bytes(lock.read_bytes())
    subprocess.run(['cargo', '+nightly-2026-09-25', 'run', '--offline', '--manifest-path',
                    str(work / 'Cargo.toml'), '--', str(work / 'packed-spec.bin')],
                   check=True, env=dict(os.environ))
    result = {'sdk_elf_sha256': hashes, 'whole_loader_capdl_report_reproduced': True,
              'rebuilt_sdk_components_equal': True, 'compiled_tool_equal': True,
              'packed_spec_sha256': hashlib.sha256(matches[0]).hexdigest(),
              'packed_spec_bytes': len(matches[0]), 'packed_spec_physical': hex(physical),
              'actual_archive_nc_caps': 8, 'actual_archive_objects': 24533}
    (out / 'sdk-payload-audit.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
