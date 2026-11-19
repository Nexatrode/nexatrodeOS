#!/usr/bin/env python3
"""Retained eight-frame SMMU installation with real CPU RAM ownership.

All MMIO is host RAM simulation. Build the actual AArch64 seL4 library too;
neither compilation nor this model proves a physical receive installation.
"""
from pathlib import Path
import hashlib, json, os, re, shutil, subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-receive-install'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(argv, label, env=None):
    p = subprocess.run(argv, cwd=ROOT, env=env, capture_output=True, text=True)
    (OUT/(label+'.log')).write_text(p.stdout+p.stderr)
    assert p.returncode == 0, str(OUT/(label+'.log'))
    return p.stdout


def count(text):
    return int(re.search(r'test result: ok\. (\d+) passed;', text)[1])


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    rustc = shutil.which('rustc')
    assert rustc
    native = ROOT/'tests/sunfish_smmu_native_test.rs'
    binary = OUT/'receive-install-test'
    feature = ['--cfg', 'feature="sunfish-smmu-install-probe"']
    run([rustc, '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2', *feature,
         '--test', str(native), '-o', str(binary)], 'native-build')
    new_groups = count(run([str(binary), 'native_receive_'], 'receive-tests'))
    all_native_groups = count(run([str(binary)], 'all-native-tests'))
    program = ROOT/'tests/sunfish_smmu_program_test.rs'
    program_binary = OUT/'program-test'
    run([rustc, '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2', '--test', str(program), '-o', str(program_binary)], 'program-build')
    program_groups = count(run([str(program_binary)], 'program-tests'))

    adapter = ROOT/'rust/src/sunfish_smmu_native.rs'
    original = adapter.read_text()
    mutations = {
        'skip-handoff-revalidation': (
            'self.receive_handoff_attempted = true;\n        self.retained_receive_domain()?;',
            'self.receive_handoff_attempted = true;\n        /* missing handoff revalidation */'),
        'ignore-canonical-page-attributes': ('if supplied.as_slice() != expected.as_slice() {', 'if false {'),
        'ignore-retained-table-corruption': ('if unsafe { pointer.add(n).read_volatile() } != *value {', 'if false {'),
    }
    harness = native.read_text().replace('../rust/src/', str(ROOT/'rust/src')+'/')
    for label, (before, after) in mutations.items():
        assert original.count(before) == 1, before
        mutant = OUT/(label+'.rs')
        text = original.replace(before, after).replace('../../tests/sunfish_ipa_shared_fixture.rs', str(ROOT/'tests/sunfish_ipa_shared_fixture.rs'))
        mutant.write_text(text)
        test = OUT/(label+'-test.rs')
        test.write_text(harness.replace(str(adapter), str(mutant)))
        target = OUT/(label+'-test')
        run([rustc, '--edition=2024', '-A', 'unused_variables', '-C', 'opt-level=2', *feature,
             '--test', str(test), '-o', str(target)], label+'-build')
        result = subprocess.run([str(target), 'native_receive_'], cwd=ROOT, capture_output=True, text=True)
        (OUT/(label+'.log')).write_text(result.stdout+result.stderr)
        assert result.returncode != 0, label+' survived'

    for script in ['audit-ipa-root-source.py', 'audit-native-ipa-shared-source.py']:
        run(['python3', str(ROOT/'platforms/sunfish'/script), '--output', str(OUT/(script+'.json'))], script)
    env = dict(os.environ,
        SEL4_INCLUDE_DIRS=str(ROOT/'.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish/board/sunfish/debug/include'),
        CARGO_TARGET_DIR=str(ROOT/'.cache/sunfish-native-wifi/private/preflight-target'))
    run(['cargo', '+nightly-2026-09-25', 'build', '--locked', '--offline', '--release', '--lib',
         '--manifest-path', 'rust/Cargo.toml', '--features', 'sunfish-smmu-install-probe',
         '--target', 'rust/targets/aarch64-sel4-microkit-pixel4k.json', '-Z', 'build-std=core,alloc', '-Z', 'json-target-spec'],
        'aarch64-sel4-library', env)
    target = Path(env['CARGO_TARGET_DIR'])/'aarch64-sel4-microkit-pixel4k/release'
    for name in ['libpersonal_os_rust_server.rlib', 'libpersonal_os_rust_server.d']:
        shutil.copy2(target/name, OUT/name)
    selected = {'rust/src/lib.rs', 'rust/Cargo.toml', 'rust/Cargo.lock',
        'rust/src/sunfish_ipa_linux_receive_native.rs', 'rust/src/sunfish_ipa_linux_receive_domain.rs',
        'rust/src/sunfish_smmu_exec.rs', 'rust/src/sunfish_smmu_program.rs', 'rust/src/sunfish_smmu_native.rs',
        'tests/sunfish_smmu_native_test.rs', 'tests/sunfish_smmu_program_test.rs', 'tests/sunfish_ipa_shared_fixture.rs',
        'platforms/sunfish/test-native-ipa-linux-receive-install.py', 'platforms/sunfish/audit-ipa-root-source.py',
        'platforms/sunfish/audit-native-ipa-shared-source.py', 'docs/sunfish-ipa-receive-plan.md'}
    for test in [native, program]:
        for path in re.findall(r'#\[path\s*=\s*"([^"]+)"\]', test.read_text()):
            selected.add((test.parent/path).resolve().relative_to(ROOT).as_posix())
    # Record actual library compiler inputs as well as the host-test closure.
    for line in (OUT/'libpersonal_os_rust_server.d').read_text().splitlines():
        if ': ' not in line:
            continue
        for name in line.split(': ', 1)[1].split():
            p = Path(name)
            if not p.is_absolute(): p = ROOT/p
            if p.is_file() and p.is_relative_to(ROOT): selected.add(p.relative_to(ROOT).as_posix())
    for rel in sorted(selected):
        p = OUT/'source'/rel
        p.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT/rel, p)
    manifest = {'source_sha256': {rel: sha(ROOT/rel) for rel in sorted(selected)},
        'new_receive_groups': new_groups, 'all_native_groups': all_native_groups,
        'program_executor_groups': program_groups, 'mutations_rejected': list(mutations),
        'aarch64_sel4_library_build': True, 'actual_receive_ram_retained_in_model': True,
        'library_exports_geometry_and_backend': True, 'eight_table_publication': True,
        'hardware_commands': [], 'actual_capdl_allocation': False, 'physical_receive_installation': False,
        'scope': 'Actual retained native backend and install method with host RAM MMIO, plus AArch64 seL4 library build; no device RX, DMA or Wi-Fi proof.'}
    manifest['artifacts'] = {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p)} for p in OUT.iterdir() if p.is_file() and p.name != 'manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    print(f'PASS: {new_groups} receive install groups, {all_native_groups} native/{program_groups} executor groups, three rejected mutations and AArch64 seL4 library; {OUT}')


if __name__ == '__main__':
    main()
