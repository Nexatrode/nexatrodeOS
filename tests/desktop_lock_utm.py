#!/usr/bin/env python3
"""Build/run the lock-gated dynamic GUI on an isolated UTM VM with a fresh disk.

Prepared driver/config artifacts are read only. This never imports the regular
VM's vault ELF, image, wallet, chats or installation key.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
from desktop_graphical_utm import patch_channels, run, ROOT, BOARD, CONFIG


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('inputs', 'surfaces', 'base', 'storage', 'composer', 'output'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--build-only', action='store_true')
    p.add_argument('--control-skip-scrub', action='store_true')
    p.add_argument('--wrapped', action='store_true', help='integrate the disposable passphrase-wrapped vault key')
    p.add_argument('--private-notes', action='store_true', help='give the dynamic Notes app private encrypted storage and test interrupted saves')
    p.add_argument('--simplex-session', type=Path, metavar='GATED_AGENT', help='restart the trusted SimpleX service group with the dynamic lock (disposable offline fixture)')
    p.add_argument('--simplex-live', action='store_true', help='enable real relay traffic only in the disposable SimpleX fixture')
    p.add_argument('--control-skip-evict', action='store_true', help='deliberately retain the provider key; requires --wrapped')
    a = p.parse_args()
    if a.control_skip_evict and (not a.wrapped or a.control_skip_scrub): p.error('skip-evict requires --wrapped and its own test')
    if a.private_notes and (not a.wrapped or a.control_skip_evict or a.control_skip_scrub): p.error('private-notes requires --wrapped and its own fresh-disk test')
    if a.simplex_session and (not a.wrapped or a.private_notes or a.control_skip_evict or a.control_skip_scrub): p.error('simplex-session requires --wrapped and its own fresh-disk test')
    if a.simplex_live and not a.simplex_session: p.error('--simplex-live requires --simplex-session')
    idle_ms = 30000 if a.private_notes or a.simplex_session else 10000
    out = a.output.resolve()
    if out.exists() or any(q.suffix == '.utm' for q in (out, *out.parents)) or out.is_relative_to(a.inputs.resolve()):
        p.error('choose a fresh build directory outside VM bundles and prepared inputs')
    out.mkdir(parents=True)
    inputs, base, storage = [x.resolve() for x in (a.inputs, a.base, a.storage)]
    sdk = inputs / 'sdk'; board = sdk / 'board' / BOARD / CONFIG
    env = dict(os.environ, SEL4_PREFIX=str(inputs / 'sel4-install'), SEL4_INCLUDE_DIRS=str(board / 'include'),
               CARGO_TARGET_DIR=str(ROOT / 'build/desktop-graphical-cargo'), PATH='/opt/homebrew/opt/llvm/bin:' + os.environ['PATH'])
    def cargo(manifest, features, bins, target):
        run('cargo', '+nightly-2026-09-25', 'build', '--locked', '--offline', '--release', '-Zjson-target-spec',
            '--config', ROOT / 'rust/.cargo/config.toml', '--manifest-path', ROOT / manifest,
            '--target', ROOT / 'rust/targets/aarch64-sel4-microkit.json', '--features', features,
            *[x for b in bins for x in ('--bin', b)], env=dict(env, CARGO_TARGET_DIR=str(ROOT / target)))
        return ROOT / target / 'aarch64-sel4-microkit/release'
    # Feature sets produce different Slint exports, so build them separately.
    target = cargo('experiments/slint-notes/Cargo.toml', 'desktop-wrapped-lock-ui' if a.wrapped else 'desktop-lock-ui', ['slint_lifecycle_lock'], 'build/desktop-graphical-cargo')
    shutil.copyfile(target / 'slint_lifecycle_lock.elf', out / 'auth.elf')
    target = cargo('experiments/slint-notes/Cargo.toml', 'desktop-private-notes' if a.private_notes else 'desktop-lock-app', ['slint_surface_fixture'], 'build/desktop-graphical-cargo')
    shutil.copyfile(target / 'slint_surface_fixture.elf', out / 'gui.elf')
    (out / 'identity.bin').write_bytes(__import__('struct').pack('<QQ', 1, 1))
    run('/opt/homebrew/opt/llvm/bin/llvm-objcopy', '--update-section', f".app_identity={out / 'identity.bin'}", out / 'gui.elf')
    patch_channels(out / 'gui.elf')
    services = 'utm-simplex-live-test' if a.simplex_live else 'utm-simplex-session-test' if a.simplex_session else 'utm-notes-interruption-test' if a.private_notes else 'utm-dynamic-vault-skip-evict' if a.control_skip_evict else 'utm-dynamic-vault-lock' if a.wrapped else 'development-crypto,utm-device-lock'
    target = cargo('rust/Cargo.toml', services, ['rust_vault_disk', 'rust_entropy_rng'] + (['rust_messages_agent_app'] if a.simplex_session else []), 'build/desktop-lock-cargo')
    sections = subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-readelf', '-S', target / 'rust_vault_disk.elf'], text=True)
    assert '.utm_vault_key' not in sections, 'installation-key vault forbidden in disposable test'
    run('/opt/homebrew/opt/llvm/bin/llvm-objcopy', '--update-section', f".blk_client_config={storage / 'blk_client_vault.data'}", target / 'rust_vault_disk.elf', out / 'rust_vault_disk.elf')
    shutil.copyfile(target / 'rust_entropy_rng.elf', out / 'rust_entropy_rng.elf')
    if a.simplex_session: shutil.copyfile(target / 'rust_messages_agent_app.elf', out / 'rust_messages_agent_app.elf')
    for name in ('app.elf', 'placeholder.elf'):
        shutil.copyfile(base / name, out / name)
    (out / 'payload.S').write_text(''.join(
        f'.section .rodata\n.balign 8\n.global {name}_elf_start, {name}_elf_end\n'
        f'{name}_elf_start:\n.incbin "{out / (name + ".elf")}"\n{name}_elf_end:\n' for name in ('app', 'gui')))
    flags = ['/opt/homebrew/opt/llvm/bin/clang', '--target=aarch64-none-elf', '-mcpu=cortex-a53', '-mgeneral-regs-only',
             '-std=gnu11', '-O2', '-ffreestanding', '-fno-stack-protector', '-nostdlib', '-Wall', '-Wextra', '-Werror', '-I', board / 'include']
    run(*flags, '-c', out / 'payload.S', '-o', out / 'payload.o')
    if a.simplex_session:
        import desktop_simplex_fixture
        session_children = desktop_simplex_fixture.prepare(out, a.simplex_session.resolve(), cargo, flags, run, live=a.simplex_live)
        if a.simplex_live:
            import desktop_simplex_network
            desktop_simplex_network.prepare(ROOT, out, inputs, run)
    run(*flags, *(['-DDYNAMIC_PRIVATE_NOTES'] if a.private_notes else []), *(['-DDYNAMIC_WRAPPED_LOCK'] if a.wrapped else []), '-c', ROOT / 'experiments/desktop-lock/probe.c', '-o', out / 'probe.o')
    for name in ('probe',):
        run('ld.lld', '-L', board / 'lib', out / (name + '.o'),
            *([out / 'payload.o'] if name == 'manager' else []), '-lmicrokit', '-T', 'microkit.ld', '-o', out / (name + '.elf'))
    system = ET.parse(base / 'graphical.system').getroot()
    manager = system.find("protection_domain[@name='runtime_manager']")
    for pd in list(system.findall('protection_domain')):
        if pd.get('name') == 'app_2': system.remove(pd)
    for link in list(system.findall('channel')):
        if any(e.get('pd') == 'app_2' or (e.get('pd') == 'timer_driver' and e.get('id') == '0') for e in link): system.remove(link)
    for m in list(manager.findall('map')):
        if m.get('mr') in ('surface_2', 'app_input_2'): manager.remove(m)
    def mr(name, size, large=False):
        ET.SubElement(system, 'memory_region', name=name, size=hex(size), **({'page_size': '0x200000'} if large else {}))
    def mapping(pd, name, address, perms, symbol=None):
        ET.SubElement(pd, 'map', mr=name, vaddr=hex(address), perms=perms, cached='true', **({'setvar_vaddr': symbol} if symbol else {}))
    def channel(pd1, n1, pd2, n2, pp=False):
        c = ET.SubElement(system, 'channel')
        ET.SubElement(c, 'end', pd=pd1, id=str(n1), **({'pp': 'true', 'notify': 'false'} if pp else {'notify': 'true'}))
        ET.SubElement(c, 'end', pd=pd2, id=str(n2), notify='true')
    mr('auth_surface', 0x800000, True); mr('auth_heap', 0x2000000, True)
    for name in ('auth_input', 'lock_status', 'lock_transfer'): mr(name, 4096)
    mapping(manager, 'auth_input', 0x66002000, 'rw', 'input_b')
    mapping(manager, 'auth_surface', 0x70000000, 'r')
    mapping(manager, 'lock_status', 0x69000000, 'rw')
    auth = ET.SubElement(system, 'protection_domain', name='lock_auth', priority='98', budget='8000', period='10000', stack_size='0x40000')
    ET.SubElement(auth, 'program_image', path='auth.elf')
    for name, address, perms in [('auth_surface',0x60000000,'rw'),('auth_heap',0x64000000,'rw'),('auth_input',0x66000000,'rw'),('lock_status',0x67000000,'r'),('lock_transfer',0x68000000,'rw')]: mapping(auth,name,address,perms)
    probe = ET.SubElement(system, 'protection_domain', name='lock_probe', priority='97')
    ET.SubElement(probe, 'program_image', path='probe.elf')
    channel('lock_auth',0,'runtime_manager',61,True)
    channel('lock_auth',1,'runtime_manager',9)
    channel('runtime_manager',8,'lock_auth',2)
    channel('lock_probe',0,'runtime_manager',60,True)
    channel('runtime_manager',10,'lock_probe',1)
    poller = ET.SubElement(system,'protection_domain',name='timer_poller',priority='1',budget='100',period='1000')
    ET.SubElement(poller,'program_image',path='timer_poller.elf'); mapping(poller,'hvf_timer_shared',0x50000000,'rw','hvf_timer_state')
    channel('timer_poller',0,'timer_driver',0)
    channel('runtime_manager',12,'timer_driver',5,True)
    storage_system = ET.parse(storage / 'storage.system').getroot()
    wanted = {'blk_driver','blk_virt','vault','entropy_rng'}
    for pd in storage_system.findall('protection_domain'):
        if pd.get('name') in wanted: system.append(copy.deepcopy(pd))
    for link in storage_system.findall('channel'):
        if all(e.get('pd') in wanted for e in link): system.append(copy.deepcopy(link))
    used = {m.get('mr') for m in system.iter('map')}
    existing = {m.get('name') for m in system.findall('memory_region')}
    for m in storage_system.findall('memory_region'):
        if m.get('name') in used and m.get('name') not in existing: system.append(copy.deepcopy(m))
    mapping(system.find("protection_domain[@name='vault']"),'lock_transfer',0x45001000,'rw','device_lock_transfer')
    channel('lock_auth',3,'entropy_rng',0,True); channel('lock_auth',4,'vault',10,True)
    if a.wrapped:
        channel('runtime_manager',13,'vault',12,True)
        channel('lock_probe',2,'vault',4 if a.private_notes else 3,True)
        mapping(probe,'contacts_transfer' if a.private_notes else 'notes_transfer',0x50000000,'rw')
    if a.private_notes:
        channel('runtime_manager',14,'vault',3,True)
        mapping(manager,'notes_transfer',0x6c000000,'rw')
    if a.simplex_session:
        desktop_simplex_fixture.compose(system, manager, out, session_children, mapping, channel, mr, live=a.simplex_live)
    used = {m.get('mr') for m in system.iter('map')}
    for m in list(system.findall('memory_region')):
        if m.get('name') not in used: system.remove(m)
    ET.ElementTree(system).write(out / 'lock.system',encoding='utf-8',xml_declaration=True)
    run(*flags, '-I', out, '-DDYNAMIC_LOCK', f'-DLOCK_IDLE_MS={idle_ms}', *(['-DDYNAMIC_PRIVATE_NOTES'] if a.private_notes else []), *(['-DDYNAMIC_WRAPPED_LOCK'] if a.wrapped else []), *(['-DDYNAMIC_SIMPLEX_SESSION'] if a.simplex_session else []), *(['-DDYNAMIC_SIMPLEX_LIVE'] if a.simplex_live else []), *(['-DCONTROL_SKIP_SCRUB'] if a.control_skip_scrub else []),
        '-c', ROOT / 'experiments/desktop-graphical/manager.c', '-o', out / 'manager.o')
    run('ld.lld', '-L', board / 'lib', out / 'manager.o', out / 'payload.o', '-lmicrokit', '-T', 'microkit.ld', '-o', out / 'manager.elf')
    if a.simplex_live: run(sys.executable, ROOT/'tools/check-hvf-network-layout.py', out/'lock.system', out)
    run(a.composer.resolve(), out/'lock.system','--board',BOARD,'--config',CONFIG,
        '--search-path',out,'--search-path',a.surfaces.resolve(),'--search-path',a.surfaces.resolve()/'gpu',
        '--search-path',storage,'--search-path',inputs/'pds', '-o',out/'loader.img','-r',out/'report.txt',env=dict(os.environ,MICROKIT_SDK=str(sdk)))
    digest=lambda x:hashlib.sha256(x.read_bytes()).hexdigest()
    sources=[ROOT/'experiments/desktop-graphical/manager.c',ROOT/'experiments/desktop-runtime/root.c',ROOT/'experiments/desktop-surfaces/compositor.c',ROOT/'experiments/desktop-surfaces/app.rs',*sorted((ROOT/'experiments/desktop-lock').glob('*')),ROOT/'experiments/slint-notes/ui/lifecycle_lock.slint',ROOT/'experiments/slint-notes/build.rs',ROOT/'experiments/slint-notes/Cargo.toml',ROOT/'rust/Cargo.toml',ROOT/'rust/src/lib.rs',ROOT/'rust/src/device_lock.rs',ROOT/'rust/src/vault_disk.rs',ROOT/'rust/src/entropy_rng.rs',ROOT/'rust/src/vault_crypto.rs',ROOT/'rust/src/vault_lifecycle.rs',ROOT/'rust/src/vault_key_prototype.rs',ROOT/'rust/src/vault_protocol.rs',ROOT/'rust/src/wrapped_vault_bootstrap.rs',ROOT/'tests/desktop_lock_input.py',ROOT/'tests/utm_vault_key_probe.py',Path(__file__).resolve()]
    if a.private_notes: sources += [*sorted((ROOT/'experiments/desktop-notes').glob('*')), ROOT/'tests/desktop_notes_input.py', ROOT/'tests/desktop_notes_broker_test.c', ROOT/'tests/desktop_lock_stub/microkit.h', ROOT/'experiments/slint-notes/ui/notes.slint']
    if a.simplex_session: sources += [*sorted((ROOT/'experiments/desktop-simplex').glob('*')), ROOT/'tests/desktop_simplex_fixture.py', ROOT/'tests/desktop_simplex_input.py', ROOT/'rust/src/vault_app_test_common.rs', ROOT/'rust/src/messages_app.rs', ROOT/'experiments/slint-notes/src/photo_reader.rs', ROOT/'experiments/slint-notes/src/photo_worker.rs', ROOT/'experiments/slint-notes/src/transcript_worker.rs']
    if a.simplex_live: sources += [ROOT/'tests/desktop_simplex_network.py', ROOT/'tests/desktop_simplex_live_input.py', ROOT/'tests/simplex_live_peer.py', *sorted((ROOT/'tests/net').glob('*.c')), *sorted((ROOT/'tests/net/include').glob('*.h'))]
    sources = [s for s in sources if s.is_file()]
    artifacts=[out/name for name in ('loader.img','manager.elf','auth.elf','gui.elf','rust_vault_disk.elf','rust_entropy_rng.elf','lock.system')]
    artifacts += [base/name for name in ('app.elf','placeholder.elf','graphical.system')]
    artifacts += [storage/name for name in ('blk_driver.elf','blk_virt.elf','blk_client_vault.data')]
    if a.simplex_live: artifacts += [out/name for name in (*desktop_simplex_network.IMAGES,'dhcp_client.elf')]
    artifacts += [a.composer.resolve(),board/'elf/sel4.elf']
    if a.simplex_session: artifacts += [out/name for name in (*desktop_simplex_fixture.IMAGES,'session_layout.h','session-layout.json')]
    (out/'build.json').write_text(json.dumps({'loader_sha256':digest(out/'loader.img'),'live_relay':a.simplex_live,'test_idle_ms':idle_ms,'private_notes':a.private_notes,'saved_data_used':False,'development_storage_key':not a.wrapped,'wrapped_random_storage_key':a.wrapped,'control_skip_evict':a.control_skip_evict,'control_skip_scrub':a.control_skip_scrub,'sources':{str(s.relative_to(ROOT)):digest(s) for s in sources},'artifacts':{str(s):digest(s) for s in artifacts}},indent=2)+'\n')
    if not a.build_only:
        run(sys.executable,ROOT/'tests/utm_vault_key_probe.py','--template',inputs/'template.plist','--loader',out/'loader.img',
            '--blank-gpt','--serial-output',out/'serial.log','--screenshot-output',out/'screen.ppm',
            '--expect','SIMPLEX_SESSION|BOOT_LOCKED' if a.simplex_session else 'LOCK_AUTH|SETUP','--reject','LOCK|FAIL|','--reject','RUNTIME|FAIL|','--reject','MON|ERROR','--reject','panicked at',
            *(['--desktop-simplex-live-test'] if a.simplex_live else ['--desktop-simplex-test'] if a.simplex_session else ['--desktop-notes-test'] if a.private_notes else ['--desktop-lock-test','skip-evict' if a.control_skip_evict else 'wrapped' if a.wrapped else 'skip-scrub' if a.control_skip_scrub else 'normal']))

if __name__ == '__main__': main()
