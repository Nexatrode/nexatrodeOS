#!/usr/bin/env python3
"""CPU-only 4/64 KiB loader comparison with the pinned original Linux loader.

No device access. A larger target work budget changes guard/service spacing,
not scalar memory operations, barriers, full verification, or PAS ordering.
Private reference firmware and generated RAM images stay in a private cache.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/mpss-target-budget-check-v1'
FEATURE = 'sunfish-mpss-target-chunk-64k'
TOOLCHAIN = 'nightly-2026-09-25'


def sha(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def run(args, label, env):
    result = subprocess.run(args, cwd=ROOT, env=env, text=True, capture_output=True)
    (OUT / (label + '.log')).write_text(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError(f'{label} failed; see {OUT / (label + ".log")}')
    return result.stdout


def main():
    os.umask(0o077)
    OUT.mkdir(mode=0o700, parents=True, exist_ok=True)
    if OUT.stat().st_mode & 0o077:
        raise ValueError('Output must be private mode 0700')
    inputs = ['rust/Cargo.toml', 'rust/src/pixel_mpss_auth.rs',
              'rust/src/sunfish_mpss_linux_loader.rs', 'rust/src/sunfish_mpss_linux_memory.rs',
              'rust/src/qcom_mdt.rs', 'rust/src/qcom_mdt_packed.rs', 'rust/src/qcom_tftp.rs',
              'rust/src/sunfish_lpd.rs', 'tests/sunfish_mpss_linux_loader_test.rs',
              'tests/sunfish_mpss_linux_loader_cases.rs', 'tests/sunfish_mpss_linux_memory_test.rs',
              'platforms/sunfish/test-native-mpss-linux-loader.py',
              'platforms/sunfish/test-native-mpss-target-budget.py']
    hashes = {name: sha(ROOT / name) for name in inputs}
    spec = importlib.util.spec_from_file_location('linux_loader_oracle', ROOT / inputs[-2])
    oracle = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(oracle)
    oracle.OUT = OUT
    # Checks the pinned Linux archive and private firmware hashes, compiles
    # original MDT/PAS C functions with recording SCM stubs and stages CPU RAM.
    linux_sources, dtbs = oracle.oracle()
    env = dict(os.environ, SUNFISH_MPSS_FIRMWARE=str(oracle.FIRMWARE),
               SUNFISH_MPSS_C_METADATA=str(OUT / 'metadata.bin'),
               SUNFISH_MPSS_C_TARGET=str(OUT / 'target.bin'),
               SUNFISH_MPSS_C_TRACE=str(OUT / 'pas-trace.txt'))
    modules = ['qcom_mdt', 'qcom_mdt_packed', 'qcom_tftp', 'sunfish_lpd',
               'sunfish_mpss_linux_loader', 'sunfish_mpss_linux_memory']
    wrapper = OUT / 'native.rs'
    wrapper.write_text('#![no_std]\n' + '\n'.join(
        f'#[path="{ROOT}/rust/src/{name}.rs"]pub mod {name};' for name in modules) + '''
use sunfish_mpss_linux_loader::{Backend, Error, Guard, Request, Phase, Loader};
pub struct Platform { pub guard: fn(Guard)->Result<(),Error>, pub acquire: fn()->Result<u64,Error>,
 pub invoke: fn(&Request)->Result<[u64;8],Error>, pub release: fn(u64)->Result<(),Error> }
impl Backend for Platform { type Lease=u64;
 fn guard(&mut self,s:Guard)->Result<(),Error>{(self.guard)(s)}
 fn acquire(&mut self)->Result<u64,Error>{(self.acquire)()}
 fn invoke(&mut self,r:&Request)->Result<[u64;8],Error>{(self.invoke)(r)}
 fn release(&mut self,l:u64)->Result<(),Error>{(self.release)(l)} }
pub fn step(l:&mut Loader<'_,sunfish_mpss_linux_memory::Window<'_>>,p:&mut Platform)
 ->Result<Phase,Error>{l.step(p)}
''')
    validation = {}
    for name, cfg, steps in [('4k', [], 77030), ('64k', ['--cfg', f'feature="{FEATURE}"'], 4844)]:
        flags = ['rustc', '+' + TOOLCHAIN, '--edition=2024', '-D', 'warnings', '-O', *cfg]
        binary = OUT / ('tests-' + name)
        run(flags + ['--test', str(ROOT / 'tests/sunfish_mpss_linux_loader_test.rs'),
                     '-o', str(binary)], name + '-build', env)
        regular = run([str(binary)], name + '-tests', env)
        assert '18 passed; 0 failed' in regular
        real = run([str(binary), 'mpss_real_private', '--ignored', '--nocapture',
                    '--test-threads=1'], name + '-private-image', env)
        assert f'target_steps={steps} bytes_written=157744117 bytes_verified=157744117' in real
        manifest = OUT / 'Cargo.toml'
        manifest.write_text('[package]\nname="mpss-target-budget-check"\nversion="0.0.0"\nedition="2024"\n'
                            '[lib]\npath="native.rs"\n[features]\n' + FEATURE + '=[]\n[workspace]\n')
        target_env = dict(env, CARGO_TARGET_DIR=str(OUT / 'target'))
        run(['cargo', '+' + TOOLCHAIN, 'build', '--offline', '--release', '--lib',
             '--manifest-path', str(manifest), '--target', 'aarch64-unknown-none',
             '-Z', 'build-std=core', *(['--features', FEATURE] if cfg else [])],
            name + '-aarch64', target_env)
        obj = OUT / 'target/aarch64-unknown-none/release/libmpss_target_budget_check.rlib'
        validation[name] = dict(host_tests=18, private_image_comparison='passed',
                                target_steps=steps, native_rlib_sha256=sha(obj))
    if any(sha(ROOT / name) != digest for name, digest in hashes.items()):
        raise ValueError('Source changed during validation')
    result = dict(result='passed', source_sha256=hashes, linux_source_sha256=linux_sources,
                  linux_commit=oracle.PIN, firmware_sha256=oracle.FIRMWARE_HASH, dtbs=dtbs,
                  validation=validation, target_image_sha256=sha(OUT / 'target.bin'),
                  metadata_sha256=sha(OUT / 'metadata.bin'), pas_trace_sha256=sha(OUT / 'pas-trace.txt'),
                  device_actions=[], limitation='Guard/service intervals increase to 64 KiB; no hardware timing or fault-latency proof.')
    (OUT / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print('PASS: both budgets, 18 host tests each, complete target/metadata/PAS trace vs pinned Linux C, AArch64 concrete loader libraries; no device action.')


if __name__ == '__main__':
    main()
