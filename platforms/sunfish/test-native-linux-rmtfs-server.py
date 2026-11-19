#!/usr/bin/env python3
"""Selected RMTFS resources through real FIFO/QRTR dispatch, offline only."""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs-server'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(args, label, env=None, success=True):
    r = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)
    (OUT / (label + '.log')).write_text(r.stdout + r.stderr)
    if success and r.returncode:
        raise SystemExit(f'{label}: exit {r.returncode}; see {OUT / (label + ".log")}')
    return r


def oracle():
    sources = {}
    folder = ROOT / '.cache/sunfish-mobian-comparison/wifi-preparation/source'
    for archive, digest, names in [
        ('rmtfs_1.1.orig.tar.gz', '3d9d37cd573a44acf9c10897b59a3f9a359c0bcf2f02d9e2b4605f6cdf4ed5c5', ['qmi_rmtfs.c', 'qmi_rmtfs.h']),
        ('qrtr_1.1.orig.tar.gz', '22da6b266c520f33edf0e70c478f986ffa3d2d7473711d089de9088b829c16b7', ['lib/qmi.c', 'include/libqrtr.h'])]:
        if sha(folder / archive) != digest:
            raise ValueError('Wrong pinned userspace archive: ' + archive)
        with tarfile.open(folder / archive) as t:
            for name in names:
                member = next(m for m in t if m.name.endswith('/' + name))
                data = t.extractfile(member).read()
                dest = OUT / 'reference' / Path(name).name
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_bytes(data)
                sources[Path(name).name] = sha(dest)
    include = OUT / 'reference'
    (include / 'linux').mkdir(exist_ok=True)
    # No QRTR sockets or logging are called by this original encoder oracle.
    (include / 'linux/qrtr.h').write_text('#include <stdint.h>\ntypedef uint32_t __le32;\n')
    (include / 'logging.h').write_text('#define LOGW(...) ((void)0)\n#define LOGE(...) ((void)0)\n')
    (OUT / 'wire.c').write_text(r'''
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "qmi_rmtfs.h"
static void emit(const char *name,int type,int id,const void *value,struct qmi_elem_info *ei){
 DEFINE_QRTR_PACKET(p,4096);
 assert(qmi_encode_message(&p,type,id,0x1234,value,ei)>0);
 printf("%s ",name);for(size_t i=0;i<p.data_len;i++)printf("%02x",((unsigned char*)p.data)[i]);puts("");
}
int main(void){
 struct rmtfs_open_resp o={.caller_id_valid=true,.caller_id=0};
 struct rmtfs_alloc_buf_resp a={.buff_address_valid=true,.buff_address=0xfde01000};
 struct rmtfs_dev_error_resp d={.status_valid=true,.status=0};
 struct rmtfs_close_resp c={0};
 struct rmtfs_dev_error_resp e={.result={.result=1,.error=1}};
 emit("open",QMI_RESPONSE,1,&o,rmtfs_open_resp_ei);
 emit("alloc",QMI_RESPONSE,4,&a,rmtfs_alloc_buf_resp_ei);
 emit("status",QMI_RESPONSE,5,&d,rmtfs_dev_error_resp_ei);
 emit("close",QMI_RESPONSE,2,&c,rmtfs_close_resp_ei);
 emit("failure",QMI_RESPONSE,5,&e,rmtfs_dev_error_resp_ei);
 struct rmtfs_alloc_buf_req ra={.caller_id=0,.buff_size=0x200000};
 struct rmtfs_iovec_req ri={.caller_id=0,.direction=0,.iovec_len=1,.iovec={{.sector_addr=0,.phys_offset=0xfde01000,.num_sector=8}},.is_force_sync=0};
 emit("alloc_req",QMI_REQUEST,4,&ra,rmtfs_alloc_buf_req_ei);
 emit("read_req",QMI_REQUEST,3,&ri,rmtfs_iovec_req_ei);
 return 0;
}
''')
    run(['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-Wno-sign-compare', '-Wno-unused-parameter',
         '-I', str(include), str(OUT / 'wire.c'), str(include / 'qmi.c'), str(include / 'qmi_rmtfs.c'),
         '-o', str(OUT / 'wire')], 'wire-build')
    (OUT / 'wire.txt').write_text(run([str(OUT / 'wire')], 'wire-exec').stdout)
    return sources


def paths(text):
    return re.sub(r'#\[path\s*=\s*"([^"]+)"\]',
                  lambda m: '#[path="' + str((ROOT / 'tests' / m[1]).resolve()) + '"]', text)


def flat_source():
    def expand(name):
        text = (ROOT / 'tests' / name).read_text()
        return re.sub(r'include!\("([^"]+)"\);', lambda m: expand(m[1]), text)
    return paths(expand('sunfish_linux_rmtfs_server_test.rs'))


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    run(['python3', 'platforms/sunfish/test-native-linux-rmtfs.py'], 'resource-reference')
    references = oracle()
    env = dict(os.environ, RUST_MIN_STACK='16777216',
               SUNFISH_LINUX_RMTFS_C=str(ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs/linux-scm.txt'),
               SUNFISH_RMTFS_WIRE_C=str(OUT / 'wire.txt'))
    rustc = shutil.which('rustc')
    flags = [rustc, '--edition=2024', '-D', 'warnings', '--cfg', 'feature="sunfish-linux-ipa-qmi"', '--cfg', 'feature="sunfish-linux-rmtfs"']
    test = ROOT / 'tests/sunfish_linux_rmtfs_server_test.rs'
    run(flags + ['--test', str(test), '-o', str(OUT / 'test')], 'rust-build', env)
    r = run([str(OUT / 'test'), 'rmtfsserver_'], 'server-tests', env)
    groups = int(re.search(r'test result: ok\. (\d+) passed;', r.stdout)[1])
    r = run([str(OUT / 'test')], 'all-tests', env)
    total = int(re.search(r'test result: ok\. (\d+) passed;', r.stdout)[1])
    # Compile real volatile FIFO + RMTFS + verified RAM-shadow ownership.
    fifo = (ROOT / 'tests/sunfish_glink_fifo_test.rs').read_text().split('use qcom_glink_fifo::', 1)[0]
    names = ['qcom_modem_handlers', 'qcom_modem_owner', 'qcom_modem_storage', 'qcom_modem_ram_storage',
             'sunfish_lpd', 'sunfish_linux_rmtfs', 'sunfish_linux_rmtfs_native', 'sunfish_linux_rmtfs_server',
             'sunfish_ipa_linux_config', 'sunfish_ipa_linux_memory', 'sunfish_ipa_linux_qmi']
    wrapper = OUT / 'native.rs'
    wrapper.write_text('#![no_std]\n' + paths(fifo) + '\n' + '\n'.join(f'#[path="{ROOT}/rust/src/{n}.rs"]pub mod {n};' for n in names) + r'''
pub struct FifoGate(pub fn()->Result<qcom_glink_fifo::Observation,qcom_glink_fifo::Fault>);
impl qcom_glink_native_io::Gate for FifoGate{fn observe(&mut self)->Result<qcom_glink_fifo::Observation,qcom_glink_fifo::Fault>{(self.0)()}}
pub struct PoolGate(pub fn()->Result<sunfish_linux_rmtfs::Observation,qcom_modem_handlers::Fault>);
impl sunfish_linux_rmtfs_native::Gate for PoolGate{fn observe(&mut self)->Result<sunfish_linux_rmtfs::Observation,qcom_modem_handlers::Fault>{(self.0)()}}
pub struct Smc(pub fn(&sunfish_linux_rmtfs::Call)->Result<[u64;8],qcom_modem_handlers::Fault>);
impl sunfish_linux_rmtfs_native::Smc for Smc{fn invoke(&mut self,c:&sunfish_linux_rmtfs::Call)->Result<[u64;8],qcom_modem_handlers::Fault>{(self.0)(c)}}
pub struct Irqs{pub ack:fn(u8)->Result<(),qcom_modem_owner::Error>,pub schedule:fn()->Result<(),qcom_modem_owner::Error>}
impl qcom_modem_owner::Interrupts for Irqs{fn acknowledge(&mut self,c:u8)->Result<(),qcom_modem_owner::Error>{(self.ack)(c)}fn schedule(&mut self)->Result<(),qcom_modem_owner::Error>{(self.schedule)()}}
pub type Owner=qcom_modem_owner::Owner<qcom_glink_native_io::NativeIo<FifoGate>>;
pub type Server=sunfish_linux_rmtfs_server::Server<'static,sunfish_linux_rmtfs_native::Native<PoolGate,Smc>>;
pub fn activate(o:&mut Owner,s:&mut Server,i:&mut Irqs)->Result<(),qcom_modem_owner::Error>{o.activate_linux_rmtfs(s,i)}
pub fn step(o:&mut Owner,s:&mut Server,i:&mut Irqs)->Result<(),qcom_modem_owner::Error>{o.notified(8,i)?;o.dispatch_linux_rmtfs_step(s,i)?;o.service_linux_ipa_step(0,i)?;Ok(())}
''')
    run(flags + ['--crate-type=lib', '--target=aarch64-unknown-none', str(wrapper), '-o', str(OUT / 'native-aarch64.rlib')], 'aarch64-native', env)
    parent = ROOT / 'rust/src/qcom_modem_owner.rs'
    child = ROOT / 'rust/src/sunfish_linux_rmtfs_owner.rs'
    core = ROOT / 'rust/src/sunfish_linux_rmtfs_server.rs'
    originals = {p.name: p.read_text() for p in [parent, child, core]}
    mutations = {
        'wrong-node-authority': (core.name, 'context.peer.node != self.config.peer_node', 'false'),
        'skip-later-vector-validation': (core.name, 'for (i, v) in iovecs.iter().enumerate() {', 'for (i, v) in iovecs.iter().take(1).enumerate() {'),
        'replay-transfer-on-backpressure': (core.name, 'if job.response.is_some() {', 'if false && job.response.is_some() {'),
        'omit-ram-sync': (core.name, 'if sync {', 'if false && sync {'),
        'discard-open-identity': (core.name, 'if let Some(f) = self.files.iter().flatten().find(|f| f.partition == p)', 'if let Some(f) = self.files.iter().flatten().find(|_| false)'),
        'accept-server-rebinding': (core.name, 'identity == 0 || self.binding.is_some()', 'identity == 0'),
        'queue-without-dual-completion': (child.name, 'Ok(ServiceProgress::WaitingReply)', '{s.reply_queued(&handle).map_err(Error::LinuxRmtfs)?;Ok(ServiceProgress::ReplyQueued)}'),
    }
    flat = flat_source()
    for label, (name, before, after) in mutations.items():
        if originals[name].count(before) != 1:
            raise ValueError('Mutation not unique: ' + label)
        folder = OUT / label
        folder.mkdir(exist_ok=True)
        for n, text in originals.items():
            (folder / n).write_text(text.replace(before, after) if n == name else text)
        shutil.copyfile(ROOT / 'rust/src/sunfish_ipa_linux_qmi_owner.rs', folder / 'sunfish_ipa_linux_qmi_owner.rs')
        source = folder / 'test.rs'
        text = flat.replace(str(parent), str(folder / parent.name)).replace(str(core), str(folder / core.name))
        source.write_text(text)
        exe = folder / 'test'
        run(flags + ['--test', str(source), '-o', str(exe)], label + '-build', env)
        if run([str(exe), 'rmtfsserver_'], label, env, success=False).returncode == 0:
            raise ValueError('Mutation accepted: ' + label)
    sdk = ROOT / '.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    native_env = dict(env, SEL4_INCLUDE_DIRS=str(sdk / 'board/sunfish/debug/include'),
        CARGO_TARGET_DIR=str(ROOT / '.cache/sunfish-native-wifi/private/preflight-target'),
        SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(ROOT / '.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo', '+nightly-2026-09-25', 'build', '--locked', '--offline', '--release', '--lib', '--manifest-path', 'rust/Cargo.toml',
         '--features', 'sunfish-native-startup-ipa-receive,sunfish-linux-rmtfs', '--target', 'rust/targets/aarch64-sel4-microkit-pixel4k.json',
         '-Z', 'build-std=core,alloc', '-Z', 'json-target-spec'], 'sel4-library', native_env)
    selected = set(re.findall(r'#\[path="([^"]+)"\]', flat))
    selected.update(str(ROOT / 'rust/src' / (name + '.rs')) for name in names)
    selected.update(str(ROOT / name) for name in ['rust/src/sunfish_linux_rmtfs_owner.rs', 'rust/src/sunfish_ipa_linux_qmi_owner.rs',
        'rust/src/sunfish_ipa_linux_qmi_fifo.rs', 'rust/src/sunfish_ipa_linux_qmi_link.rs', 'rust/src/lib.rs', 'rust/Cargo.toml', 'rust/Cargo.lock',
        'platforms/sunfish/test-native-linux-rmtfs-server.py', 'docs/sunfish-native-wifi-startup.md'])
    selected.update(str(p) for p in (ROOT / 'tests').glob('sunfish_linux_rmtfs*rs'))
    selected.add(str(ROOT / 'tests/sunfish_ipa_linux_qmi_owner_test.rs'))
    source_hashes = {}
    for path in sorted(map(Path, selected)):
        name = str(path.relative_to(ROOT))
        dest = OUT / 'source' / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, dest)
        source_hashes[name] = sha(path)
    artifacts = [p for p in OUT.iterdir() if p.is_file() and p.name != 'manifest.json']
    artifacts += [p for label in mutations for p in (OUT / label).iterdir() if p.is_file()]
    upstream = ROOT / '.cache/sunfish-native-wifi/private/linux-rmtfs/manifest.json'
    shutil.copyfile(upstream, OUT / 'resource-reference.json')
    artifacts = sorted(set(artifacts + [OUT / 'resource-reference.json']))
    manifest = {'server_behavioral_groups': groups, 'existing_regressions_with_feature': total - groups,
                'default_resource_and_owner_regressions_rerun': True, 'reference_source_sha256': references,
                'source_sha256': source_hashes, 'mutations_rejected': list(mutations),
                'aarch64_native_fifo_resource_server_monomorphized': True, 'sel4_library_built': True,
                'device_actions': [], 'upstream_manifest_sha256': sha(upstream),
                'reference_artifacts': {str(p.relative_to(OUT)): {'bytes': p.stat().st_size, 'sha256': sha(p)} for p in (OUT / 'reference').rglob('*') if p.is_file()},
                'scope': 'Actual QRTR/FIFO/owner request and reply lifecycle coupled to pre-shared selected RMTFS resources and verified RAM shadows. Source descriptors and original userspace encoder oracle. Native capability/mapping factory, composed MPSS startup and Wi-Fi remain unproven.',
                'artifacts': {str(p.relative_to(OUT)): {'bytes': p.stat().st_size, 'sha256': sha(p)} for p in artifacts}}
    (OUT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'PASS: {groups} storage/owner groups, {total-groups} regressions, seven rejected mutations, original userspace wire oracle, AArch64 native composition and seL4 library; no device action.')


if __name__ == '__main__':
    main()
