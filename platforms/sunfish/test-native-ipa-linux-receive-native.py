#!/usr/bin/env python3
"""CPU-only validation of the exclusive receive-RAM backend; no device I/O."""
from pathlib import Path
import hashlib, json, re, shutil, subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-receive-native'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(argv, name):
    p = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True)
    (OUT/(name+'.log')).write_text(p.stdout+p.stderr)
    assert p.returncode == 0, str(OUT/(name+'.log'))
    return p.stdout


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    rustc = shutil.which('rustc')
    assert rustc
    binary = OUT/'receive-native-test'
    test = ROOT/'tests/sunfish_ipa_linux_receive_native_test.rs'
    run([rustc, '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2', '--test', str(test), '-o', str(binary)], 'rust-build')
    tested = run([str(binary), 'rxnative_'], 'rust-tests')
    groups = int(re.search(r'test result: ok\. (\d+) passed;', tested)[1])
    names = ['sunfish_ipa_linux_buffers', 'sunfish_ipa_linux_gsi', 'sunfish_ipa_linux_endpoint',
             'sunfish_ipa_linux_receive', 'sunfish_ipa_linux_receive_pool', 'sunfish_ipa_linux_receive_native']
    wrapper = OUT/'core.rs'
    wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names))
    run([rustc, '--edition=2024', '-D', 'warnings', '--crate-type=lib', '--target=aarch64-unknown-none', str(wrapper), '-o', str(OUT/'receive-native-aarch64.rlib')], 'aarch64-core')
    selected = ['rust/src/'+n+'.rs' for n in names]+['tests/sunfish_ipa_linux_receive_native_test.rs',
        'platforms/sunfish/test-native-ipa-linux-receive-native.py', 'docs/sunfish-ipa-receive-plan.md']
    for rel in selected:
        p = OUT/'source'/rel
        p.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT/rel, p)
    manifest = {'source_sha256': {rel: sha(ROOT/rel) for rel in selected}, 'behavioral_groups': groups,
        'cpu_bytes': 0x200000, 'reserved_pages': 512, 'proposed_dma_pages': 502,
        'aarch64_no_std': True, 'hardware_commands': [], 'actual_capdl_allocation': False,
        'installed_receive_mapping': False, 'library_exported': 'pub mod sunfish_ipa_linux_receive_native;' in (ROOT/'rust/src/lib.rs').read_text(),
        'scope': 'Exclusive scalar volatile CPU memory backend, host allocation and AArch64 compilation only; no physical DMA/IRQ/RX proof.'}
    manifest['artifacts'] = {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p)} for p in OUT.iterdir() if p.is_file() and p.name != 'manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    print(f'PASS: {groups} receive native-backend CPU groups, full 2 MiB span, AArch64 no_std; {OUT}')


if __name__ == '__main__':
    main()
