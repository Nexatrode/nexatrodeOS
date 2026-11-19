#!/usr/bin/env python3
"""Actual owner IRQ translation, modeled kernel, and concrete AArch64 adapter.

This new component is deliberately outside the frozen PAS4 image. Do not
change that image or claim physical IRQ delivery from this offline result.
"""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/mpss-runtime-irq'
HARNESS = ROOT / 'tests/pixel_mpss_runtime_irq_test.rs'
MODULE = ROOT / 'rust/src/pixel_mpss_runtime_irq.rs'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def deps(path):
    for a, b in re.findall(r'#\[path\s*=\s*"([^"]+)"\]|include!\("([^"]+)"\)', path.read_text()):
        yield (path.parent / (a or b)).resolve()


def absolute(text, path):
    for name in re.findall(r'"([^"]+\.rs)"', text):
        target = (path.parent / name).resolve()
        if target.is_file(): text = text.replace('"' + name + '"', json.dumps(str(target)))
    return text


def expand(path):
    text = re.sub(r'include!\("([^"]+)"\);', lambda m: expand((path.parent / m[1]).resolve()), path.read_text())
    return absolute(text, path)


def function_body(text, signature):
    start = text.index(signature)
    opening = text.index('{', start)
    depth = 0
    for index in range(opening, len(text)):
        if text[index] == '{':
            depth += 1
        elif text[index] == '}':
            depth -= 1
            if depth == 0:
                return text[start:index + 1]
    raise ValueError('unterminated pinned function: ' + signature)


def compact(text):
    return ''.join(re.sub(r'/\*.*?\*/|//[^\n]*', '', text, flags=re.S).split())


def validate_pinned_boot_order(pas_source, core_source, common_source):
    pas = compact(function_body(pas_source, 'static int qcom_pas_start(struct rproc *rproc)'))
    load = pas.index('ret=qcom_mdt_pas_load(')
    authenticate = pas.index('ret=qcom_scm_pas_prepare_and_auth_reset(pas->pas_ctx);', load)
    wait_ready = pas.index('ret=qcom_q6v5_wait_for_start(', authenticate)
    returned = pas.index('return0;', wait_ready)
    if not load < authenticate < wait_ready < returned:
        raise ValueError('PAS auth/reset must precede the delivered Q6-ready wait and callback return')

    start = compact(function_body(core_source,
        'static int rproc_start(struct rproc *rproc, const struct firmware *fw)'))
    start_callback = start.index('ret=rproc->ops->start(rproc);')
    subdevices = start.index('ret=rproc_start_subdevices(rproc);', start_callback)
    if not start_callback < subdevices:
        raise ValueError('remoteproc subdevices must start after the PAS callback returns')

    glink_start = compact(function_body(common_source,
        'static int glink_subdev_start(struct rproc_subdev *subdev)'))
    glink_add = compact(function_body(common_source,
        'void qcom_add_glink_subdev(struct rproc *rproc, struct qcom_rproc_glink *glink,'))
    if 'glink->edge=qcom_glink_smem_register(glink->dev,glink->node);' not in glink_start:
        raise ValueError('GLINK subdevice must register the SMEM edge')
    if 'glink->subdev.start=glink_subdev_start;' not in glink_add or 'rproc_add_subdev(rproc,&glink->subdev);' not in glink_add:
        raise ValueError('registered GLINK callback must be the SMEM edge subdevice')
    return dict(pas4_before_q6_ready=True, remoteproc_subdevices_after_start_callback=True,
                glink_subdevice_registers_smem=True)


def check_boot_order_mutations(pas_source, core_source, common_source):
    mutations = {
        'missing Q6 ready wait': (pas_source, core_source, common_source,
            'ret = qcom_q6v5_wait_for_start(&pas->q6v5, msecs_to_jiffies(5000));', ''),
        'subdevices before PAS callback': (pas_source, core_source, common_source,
            'ret = rproc->ops->start(rproc);', 'ret = rproc_start_subdevices(rproc);'),
        'GLINK without SMEM edge': (pas_source, core_source, common_source,
            'qcom_glink_smem_register(glink->dev, glink->node)', 'qcom_glink_register(glink->dev, glink->node)'),
    }
    rejected = []
    for label, (pas, core, common, before, after) in mutations.items():
        if before not in {'missing Q6 ready wait': pas_source,
                          'subdevices before PAS callback': core_source,
                          'GLINK without SMEM edge': common_source}[label]:
            raise ValueError('missing boot-order mutation anchor: ' + label)
        changed = {'missing Q6 ready wait': [pas.replace(before, after, 1), core, common],
                   'subdevices before PAS callback': [pas, core.replace(before, after, 1), common],
                   'GLINK without SMEM edge': [pas, core, common.replace(before, after, 1)]}[label]
        try:
            validate_pinned_boot_order(*changed)
        except ValueError:
            rejected.append(label)
        else:
            raise ValueError('pinned source-order mutation accepted: ' + label)
    return rejected


def main():
    os.umask(0o077)
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    # An earlier direct compiler check may have created this task-owned folder
    # under the shell umask. Make only this proof folder private.
    OUT.chmod(0o700)
    paths = {HARNESS, MODULE, Path(__file__).resolve(), ROOT / 'rust/Cargo.toml', ROOT / 'rust/Cargo.lock'}
    pending = [HARNESS]
    while pending:
        for path in deps(pending.pop()):
            if path not in paths: paths.add(path); pending.append(path)
    hashes = {str(p.relative_to(ROOT)): sha(p) for p in sorted(paths)}

    def run(args, name, env=None, success=True):
        result = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)
        (OUT / (name + '.log')).write_text(result.stdout + result.stderr)
        if success and result.returncode: raise SystemExit(f'{name} failed; see {OUT / (name + ".log")}')
        return result

    rustc = shutil.which('rustc')
    flags = ['--edition=2024', '-D', 'warnings', '--test']
    run([rustc, *flags, str(HARNESS), '-o', str(OUT / 'tests')], 'build')
    tests = run([str(OUT / 'tests'), '--test-threads=1'], 'tests')
    groups = len(re.findall(r'test native_irq::.* \.\.\. ok', tests.stdout))
    total = int(re.search(r'test result: ok\. (\d+) passed', tests.stdout)[1])
    if groups != 13: raise ValueError('all thirteen router/actual-owner groups required')
    changes = {
        'glink-acks-smmu-slot': ('pub const GLINK_CHANNEL: u8 = 4;', 'pub const GLINK_CHANNEL: u8 = 0;'),
        'smp-acks-smmu-slot': ('pub const SMP_CHANNEL: u8 = 5;', 'pub const SMP_CHANNEL: u8 = 1;'),
        'gsi-creates-continuation': ('self.other |= physical_badge & OTHER;', 'self.other |= physical_badge & OTHER; if physical_badge & 8 != 0 { self.continuation = true; }'),
        'ignore-smmu-fault': ('if physical_badge & 3 != 0 {', 'if false {'),
        'ack-without-delivery': ('if self.pending & self.delivered & bit == 0 {', 'if false {'),
        'retry-after-unwind': ('if self.busy {', 'if false {'),
        'leave-owner-running-after-router-fault': ('owner.fault();', ''),
    }
    for name, (before, after) in changes.items():
        text = MODULE.read_text()
        if before not in text: raise ValueError('missing mutation anchor: ' + name)
        folder = OUT / name; folder.mkdir(exist_ok=True)
        module = folder / 'module.rs'; module.write_text(text.replace(before, after))
        harness = folder / 'test.rs'
        harness.write_text(expand(HARNESS).replace(json.dumps(str(MODULE)), json.dumps(str(module))))
        run([rustc, *flags, '-A', 'dead_code', str(harness), '-o', str(folder / 'test')], name + '-build')
        if run([str(folder / 'test'), 'native_irq::', '--test-threads=1'], name, success=False).returncode == 0:
            raise ValueError('behavioral mutation accepted: ' + name)
    archive = ROOT / '.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive) != 'd55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2':
        raise ValueError('wrong selected Linux archive')
    with tarfile.open(archive) as tar:
        names = {suffix: next(m for m in tar if m.name.endswith('/' + suffix)) for suffix in [
            'arch/arm64/boot/dts/qcom/sm7150.dtsi',
            'drivers/remoteproc/qcom_q6v5_pas.c',
            'drivers/remoteproc/remoteproc_core.c',
            'drivers/remoteproc/qcom_common.c']}
        contents = {suffix: tar.extractfile(member).read() for suffix, member in names.items()}
    dts = contents['arch/arm64/boot/dts/qcom/sm7150.dtsi']
    text = dts.decode()
    if 'interrupts = <GIC_SPI 449 IRQ_TYPE_EDGE_RISING>;' not in text[text.index('remoteproc_mpss:'):]:
        raise ValueError('selected GLINK IRQ changed')
    if 'interrupts = <GIC_SPI 451 IRQ_TYPE_EDGE_RISING>;' not in text[text.index('smp2p-modem {'):]:
        raise ValueError('selected SMP2P IRQ changed')
    boot_order = validate_pinned_boot_order(
        contents['drivers/remoteproc/qcom_q6v5_pas.c'].decode(),
        contents['drivers/remoteproc/remoteproc_core.c'].decode(),
        contents['drivers/remoteproc/qcom_common.c'].decode())
    order_mutations = check_boot_order_mutations(
        contents['drivers/remoteproc/qcom_q6v5_pas.c'].decode(),
        contents['drivers/remoteproc/remoteproc_core.c'].decode(),
        contents['drivers/remoteproc/qcom_common.c'].decode())
    package = OUT / 'aarch64'; package.mkdir(exist_ok=True)
    (package / 'Cargo.toml').write_text('''[package]
name="sunfish-mpss-runtime-irq-check"
version="0.0.0"
edition="2024"
[lib]
path="native.rs"
[dependencies]
personal-os-rust-server={path=''' + json.dumps(str(ROOT / 'rust')) + ''',default-features=false,features=["sunfish-native-startup-mpss-glink"]}
sel4-microkit={git="https://github.com/seL4/rust-sel4",rev="891b59622306e94ad428193838f0d6214bb5c7aa"}
''')
    shutil.copyfile(ROOT / 'rust/Cargo.lock', package / 'Cargo.lock')
    (package / 'native.rs').write_text('''#![no_std]
#[path=''' + json.dumps(str(MODULE)) + ''']pub mod irq;
pub use personal_os_rust_server::{qcom_glink_fifo,qcom_modem_owner};
use personal_os_rust_server::{qcom_modem_owner::Owner,sunfish_mpss_linux_transport_native::Retained,sunfish_smmu_native as d};
pub struct Kernel;
impl irq::Kernel for Kernel {
 fn acknowledge(&mut self,ch:u8)->Result<(),()> {
  if ch!=irq::GLINK_CHANNEL && ch!=irq::SMP_CHANNEL {return Err(());}
  sel4_microkit::Channel::new(ch as usize).irq_ack().map_err(|_|())
 }
}
pub type NativeIo<'m>=Retained<'m,fn(usize)->Result<(),d::Error>>;
pub fn dispatch(r:&mut irq::Router<Kernel>,o:&mut Owner<NativeIo<'_>>)->Result<personal_os_rust_server::qcom_modem_owner::Progress,irq::Error>{r.dispatch(o)}
pub fn capture(r:&mut irq::Router<Kernel>,b:u64)->Result<(),irq::Error>{r.capture(b)}
''')
    sdk = ROOT / '.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    target = OUT / 'target'
    env = dict(os.environ, SEL4_INCLUDE_DIRS=str(sdk / 'board/sunfish/debug/include'),
        CARGO_TARGET_DIR=str(target), SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(OUT.parent / 'prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo', '+nightly-2026-09-25', 'build', '--offline', '--release', '--manifest-path', str(package / 'Cargo.toml'),
        '--target', str(ROOT / 'rust/targets/aarch64-sel4-microkit-pixel4k.json'), '-Z', 'build-std=core,alloc', '-Z', 'json-target-spec'], 'aarch64', env)
    original = {(p['name'],p['version'],p.get('source')):p for p in tomllib.loads((ROOT / 'rust/Cargo.lock').read_text())['package']}
    for p in tomllib.loads((package / 'Cargo.lock').read_text())['package']:
        if p['name'] != 'sunfish-mpss-runtime-irq-check' and original.get((p['name'],p['version'],p.get('source'))) != p:
            raise ValueError('changed pinned dependency: ' + p['name'])
    for name, expected in hashes.items():
        if sha(ROOT / name) != expected: raise ValueError('source changed during tests: ' + name)
        dest = OUT / 'source' / name; dest.parent.mkdir(parents=True,exist_ok=True); shutil.copyfile(ROOT / name,dest)
    result = dict(result='passed', source_sha256=hashes, native_irq_groups=groups,total_host_groups=total,
        mutations_rejected=list(changes), linux_dtsi_sha256=hashlib.sha256(dts).hexdigest(),
        linux_startup_order=boot_order, linux_startup_order_mutations_rejected=order_mutations,
        linux_startup_source_sha256={name: hashlib.sha256(contents[name]).hexdigest() for name in contents},
        physical_glink_irq=481, physical_smp2p_irq=483, future_native_channels=[4,5],
        aarch64_wrapper_sha256=sha(package / 'native.rs'),
        aarch64_rlib_sha256=sha(target / 'aarch64-sel4-microkit-pixel4k/release/libsunfish_mpss_runtime_irq_check.rlib'),
        scope='Actual serialized modem owner and router with CPU FIFO/kernel model; concrete seL4 IRQ-cap adapter compiled. Not integrated into frozen PAS4 image.',
        pinned_dependencies_unchanged=True, device_actions=[],physical_irq_delivery_proven=False,
        native_mpss_started=False,native_wifi_proven=False)
    (OUT / 'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'PASS: {groups} router groups, {total} total host groups, seven rejected mutations and concrete seL4/AArch64 adapter; no device action.')


if __name__ == '__main__':main()
