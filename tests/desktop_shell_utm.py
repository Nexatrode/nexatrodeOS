#!/usr/bin/env python3
"""Build a landscape shell and isolated apps; boot only a disposable offline VM."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import xml.etree.ElementTree as ET
from desktop_graphical_utm import patch_channels

ROOT = Path(__file__).resolve().parents[1]
BOARD, CONFIG = 'qemu_virt_aarch64_el1', 'smp-debug'

def run(*args, **kwargs):
    subprocess.run(list(map(str, args)), cwd=ROOT, check=True, **kwargs)

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--inputs', type=Path, required=True)
    p.add_argument('--surfaces', type=Path, required=True)
    p.add_argument('--base', type=Path, required=True, help='prepared dynamic graphical SDF')
    p.add_argument('--sddf', type=Path, required=True)
    p.add_argument('--composer', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--scale', type=int, choices=(1,2), default=2)
    p.add_argument('--build-only', action='store_true')
    a = p.parse_args()
    inputs, surfaces, base, out = [v.resolve() for v in (a.inputs, a.surfaces, a.base, a.output)]
    if out.exists() or not out.is_relative_to(ROOT / 'build') or out.is_relative_to(inputs):
        p.error('use a fresh build directory in this worktree')
    out.mkdir(parents=True)
    scale = a.scale
    width, height = 1280 * scale, 800 * scale
    frame_bytes = 0x1000000 if scale == 2 else 0x800000
    sdk = inputs / 'sdk'; board = sdk / 'board' / BOARD / CONFIG
    target = ROOT / 'build/desktop-shell-cargo'
    env = dict(os.environ, SLINT_FONT_SIZES=','.join(str(n) for n in (12,14,15,16,20,22,25,38,48)), SEL4_PREFIX=str(inputs / 'sel4-install'), SEL4_INCLUDE_DIRS=str(board / 'include'),
               CARGO_TARGET_DIR=str(target), PATH='/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:' + os.environ['PATH'])
    def rust(features, binary, destination):
        command = ['cargo', '+nightly-2026-09-25', 'build', '--locked', '--offline', '--release', '-Zjson-target-spec', '--message-format=json',
            '--config', str(ROOT / 'rust/.cargo/config.toml'), '--manifest-path', str(ROOT / 'experiments/slint-notes/Cargo.toml'),
            '--target', str(ROOT / 'rust/targets/aarch64-sel4-microkit.json'), '--features', features + (',desktop-shell-2x' if scale == 2 else ''), '--bin', binary]
        result = subprocess.run(command,cwd=ROOT,env=env,stdout=subprocess.PIPE,text=True,check=True)
        metadata = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
        generated = [Path(m['out_dir']) for m in metadata if m.get('reason') == 'build-script-executed' and 'personal-os-slint-notes-spike' in m.get('package_id','')]
        assert len(generated) == 1, generated
        shutil.copyfile(target / 'aarch64-sel4-microkit/release' / (binary + '.elf'), out / destination)
        return generated[0]
    rust('desktop-shell-app,desktop-dynamic-ui', 'slint_surface_fixture', 'gui.elf')
    (out / 'identity.bin').write_bytes(struct.pack('<QQ', 1, 1))
    run('/opt/homebrew/opt/llvm/bin/llvm-objcopy', '--update-section', f'.app_identity={out / "identity.bin"}', out / 'gui.elf')
    patch_channels(out / 'gui.elf')
    rust('desktop-shell-app', 'slint_surface_fixture', 'app_2.elf')
    (out / 'identity.bin').write_bytes(struct.pack('<QQ', 2, 1))
    run('/opt/homebrew/opt/llvm/bin/llvm-objcopy', '--update-section', f'.app_identity={out / "identity.bin"}', out / 'app_2.elf')
    shell_out = rust('desktop-shell-ui', 'desktop_shell', 'desktop_shell.elf')
    generated = shell_out / 'desktop_shell.rs'
    text = generated.read_text()
    families = re.findall(r'family_name : .*?\("([^"]+)"', text)
    weights = sorted(set(map(int,re.findall(r'weight : (\d+)u16',text))))
    sizes = sorted(set(map(int,re.findall(r'pixel_size : (\d+)i16',text))))
    assert families and set(families) == {'Roboto'}, families
    assert {400,600,700}.issubset(weights), weights
    assert 15 * scale in sizes, sizes
    shutil.copyfile(ROOT / 'experiments/slint-notes/assets/fonts/Roboto-OFL.txt', out / 'Roboto-OFL.txt')
    (out / 'fonts.json').write_text(json.dumps({'family':families,'weights':weights,'physical_sizes':sizes,'scale':scale,'glyph_size_for_15px_label':15*scale},indent=2)+'\n')
    flags = ['clang', '--target=aarch64-none-elf', '-mcpu=cortex-a53', '-mgeneral-regs-only', '-std=gnu11', '-O2',
             '-ffreestanding', '-fno-stack-protector', '-nostdlib', '-Wall', '-Wextra', '-Werror', '-I', board / 'include']
    runtime = ROOT / 'experiments/desktop-runtime'
    for name in ('app', 'composer_placeholder'):
        run(*flags, '-c', runtime / (name + '.c'), '-o', out / (name + '.o'), env=env)
    run('ld.lld', '-T', runtime / 'app.ld', out / 'app.o', '-o', out / 'app.elf', env=env)
    run('ld.lld', '-L', board / 'lib', out / 'composer_placeholder.o', '-lmicrokit', '-T', 'microkit.ld', '-o', out / 'placeholder.elf', env=env)
    (out / 'payload.S').write_text(''.join(f'.section .rodata\n.balign 8\n.global {n}_elf_start, {n}_elf_end\n{n}_elf_start:\n.incbin "{n}.elf"\n{n}_elf_end:\n' for n in ('app', 'gui')))
    run(*flags, '-I', out, '-c', out / 'payload.S', '-o', out / 'payload.o')
    run(*flags, '-DDESKTOP_SHELL', f'-DDESKTOP_SCALE={scale}u', f'-DSLINT_FRAME_WIDTH={width}u', f'-DSLINT_FRAME_HEIGHT={height}u', f'-DSLINT_FRAME_REGION_BYTES={frame_bytes}u', '-c', ROOT / 'experiments/desktop-graphical/manager.c', '-o', out / 'manager.o', env=env)
    run('ld.lld', '-L', board / 'lib', out / 'manager.o', out / 'payload.o', '-lmicrokit', '-T', 'microkit.ld', '-o', out / 'manager.elf', env=env)
    alias = out / 'gpu-sdk'; (alias / 'board').mkdir(parents=True)
    (alias / 'board/qemu_virt_aarch64').symlink_to(sdk / 'board' / BOARD, target_is_directory=True)
    (alias / 'bin').symlink_to(sdk / 'bin', target_is_directory=True)
    run(sys.executable, ROOT / 'experiments/ghc-microkit/build_slint_gpu_client.py', '--sdk', alias, '--sddf', a.sddf.resolve(),
        '--build', out / 'gpu-client', '--config', CONFIG, '--width', str(width), '--height', str(height), '--frame-region-bytes', hex(frame_bytes), '--cursor-scale', str(4*scale), env=env)
    shutil.copyfile(out / 'gpu-client/client.elf', out / 'gpu_client.elf')
    tree = ET.parse(base / 'graphical.system'); system = tree.getroot()
    permitted = {'timer_driver','gpu_driver','gpu_virt','gpu_client','input_driver','runtime_manager','runtime_budget','app_2'}
    assert {pd.get('name') for pd in system.iter('protection_domain')} == permitted, 'base must be the isolated graphical fixture'
    manager = system.find("protection_domain[@name='runtime_manager']")
    manager.set('priority', '105')  # PPC callers must be below the manager.
    # No storage, wallet, agent or network service is added to this shell fixture.
    def region(name, size, **kw): ET.SubElement(system, 'memory_region', name=name, size=hex(size), **kw)
    system.find("memory_region[@name='slint_frame']").set('size', hex(frame_bytes))
    system.find("memory_region[@name='surface_2']").set('size', hex(0x400000 if scale == 2 else 0x200000))
    region('shell_surface', 0x1000000 if scale == 2 else 0x400000, page_size='0x200000')
    region('shell_heap', 0x2000000, page_size='0x200000')
    region('shell_state', 0x1000); region('shell_input', 0x1000)
    def mapping(pd, mr, address, perms, symbol):
        ET.SubElement(pd, 'map', mr=mr, vaddr=hex(address), perms=perms, cached='true', setvar_vaddr=symbol)
    mapping(manager, 'shell_surface', 0x69000000, 'r', 'shell_surface')
    mapping(manager, 'shell_state', 0x6a000000, 'rw', 'shell_state')
    mapping(manager, 'shell_input', 0x6a001000, 'rw', 'shell_input')
    mapping(manager, 'ui_pointer', 0x6a002000, 'rw', 'ui_pointer')
    shell = ET.SubElement(system, 'protection_domain', name='desktop_shell', priority='100', budget='4000', period='10000', stack_size='0x40000')
    ET.SubElement(shell, 'program_image', path='desktop_shell.elf')
    for mr, addr, perms in [('shell_surface',0x60000000,'rw'), ('shell_heap',0x64000000,'rw'), ('shell_state',0x68000000,'r'), ('shell_input',0x68100000,'rw'), ('clock_view',0x68200000,'r')]:
        mapping(shell, mr, addr, perms, mr)
    def link(pd, ch, peer, peer_ch, **kw):
        c = ET.SubElement(system, 'channel'); ET.SubElement(c, 'end', pd=pd, id=str(ch), **kw); ET.SubElement(c, 'end', pd=peer, id=str(peer_ch))
    link('desktop_shell', 0, 'runtime_manager', 61, pp='true')
    link('desktop_shell', 1, 'runtime_manager', 20)
    link('runtime_manager', 21, 'desktop_shell', 3)
    link('runtime_manager', 22, 'desktop_shell', 2)
    link('runtime_manager', 23, 'app_2', 1)
    region('rtc', 0x1000, phys_addr='0x09010000')
    clock = ET.SubElement(system, 'protection_domain', name='clock', priority='101', budget='1000', period='10000')
    ET.SubElement(clock, 'program_image', path='rust_clock.elf')
    ET.SubElement(clock, 'map', mr='rtc', vaddr='0x50000000', perms='rw', cached='false', setvar_vaddr='rtc_regs')
    mapping(clock, 'clock_view', 0x50100000, 'rw', 'clock_view')
    ET.SubElement(clock, 'irq', irq='34', id='0')
    link('clock', 1, 'desktop_shell', 4)
    tree.write(out / 'desktop.system', encoding='utf-8', xml_declaration=True)
    run(a.composer.resolve(), out / 'desktop.system', '--board', BOARD, '--config', CONFIG, '--search-path', out,
        '--search-path', surfaces, '--search-path', surfaces / 'gpu', '--search-path', inputs / 'pds',
        '-o', out / 'loader.img', '-r', out / 'report.txt', env=dict(env, MICROKIT_SDK=str(sdk)))
    digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    sources = list((ROOT / 'experiments/desktop-shell').glob('*')) + [Path(__file__).resolve(), ROOT / 'experiments/desktop-graphical/manager.c', ROOT / 'experiments/desktop-surfaces/app.rs', ROOT / 'experiments/slint-notes/ui/desktop_shell.slint', ROOT / 'experiments/slint-notes/ui/desktop_app.slint', ROOT / 'tests/desktop_shell_input.py', ROOT / 'tests/desktop_shell_perf.py', ROOT / 'tests/desktop_shell_sharpness.py', ROOT / 'experiments/slint-notes/build.rs', ROOT / 'experiments/slint-notes/Cargo.toml', *list((ROOT / 'experiments/slint-notes/assets/fonts').glob('Roboto-*')), ROOT / 'tests/input/gpu_client.c', ROOT / 'tests/input/input_handoff.h', ROOT / 'experiments/desktop-surfaces/input.h', ROOT / 'experiments/desktop-runtime/root.c', ROOT / 'experiments/desktop-graphical/image.h', ROOT / 'tests/input/slint_frame.h', ROOT / 'experiments/ghc-microkit/build_slint_gpu_client.py']
    (out / 'build.json').write_text(json.dumps({'resolution': [width,height], 'logical_resolution': [1280,800], 'scale': scale, 'vcpus': 4, 'vault': False, 'network': 'restricted', 'sources': {str(p.relative_to(ROOT)): digest(p) for p in sources}, 'prepared': {str(p): digest(p) for p in [base / 'graphical.system', board / 'elf/sel4.elf', surfaces / 'input_driver.elf', inputs / 'pds/gpu_driver.elf', inputs / 'pds/gpu_virt.elf', inputs / 'pds/rust_clock.elf', a.composer.resolve()]}, 'artifacts': {p.name: digest(p) for p in out.glob('*.elf')}, 'loader_sha256': digest(out / 'loader.img')}, indent=2) + '\n')
    if not a.build_only:
        run(sys.executable, ROOT / 'tests/utm_vault_key_probe.py', '--template', inputs / 'template.plist', '--loader', out / 'loader.img', '--serial-output', out / 'serial.log', '--screenshot-output', out / 'screen.ppm', '--expect', 'Booting all finished, dropped to user space', '--reject', 'RUNTIME|FAIL|', '--reject', 'panicked at', '--reject', 'DYNAMIC|UNEXPECTED_FAULT|', '--desktop-shell-test')

if __name__ == '__main__': main()
