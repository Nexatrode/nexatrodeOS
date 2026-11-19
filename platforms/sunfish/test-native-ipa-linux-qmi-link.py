#!/usr/bin/env python3
"""Linux IPA bookkeeping composed with the real GLINK/QRTR transport core."""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi-link'
PIN = '2267e248204e7e051e0f7355298949a6440de579'
ARCHIVE_HASH = 'd55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'
FEATURE = 'feature="sunfish-linux-ipa-qmi"'
REFERENCE = ['drivers/rpmsg/qcom_glink_native.c', 'net/qrtr/af_qrtr.c', 'include/uapi/linux/qrtr.h']


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(args, label, env=None, success=True):
    result = subprocess.run(args, cwd=ROOT, env=env, text=True, capture_output=True)
    (OUT / (label + '.log')).write_text(result.stdout + result.stderr)
    if success and result.returncode:
        raise SystemExit(f'{label}: exit {result.returncode}; see {OUT / (label + ".log")}')
    return result


def rust_modules(text):
    return re.sub(r'(?<=path = ")\.\./rust/src/([^"\n]+)',
                  lambda m: str(ROOT / 'rust/src' / m[1]), text)


def wire_reference():
    archive = ROOT / '.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive) != ARCHIVE_HASH:
        raise ValueError('Wrong pinned archive')
    sources = {}
    with tarfile.open(archive) as tar:
        members = {m.name.split('/', 1)[1]: m for m in tar.getmembers() if '/' in m.name}
        for name in REFERENCE:
            source = tar.extractfile(members[name]).read().decode()
            path = OUT / 'reference' / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(source)
            sources[name] = source
    glink = sources[REFERENCE[0]]
    qrtr = sources[REFERENCE[1]]
    qmi_out = ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi'
    proof = json.loads((qmi_out / 'manifest.json').read_text())
    golden = qmi_out / 'linux-qmi.txt'
    if sha(golden) != proof['artifacts'][golden.name]['sha256']:
        raise ValueError('Changed original QMI oracle')
    qmi = bytes.fromhex(next(s.split()[1] for s in golden.read_text().splitlines() if s.startswith('cold ')))
    c = r'''
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <stdbool.h>
typedef uint8_t u8;typedef uint16_t __le16;typedef uint32_t __le32;
#define __packed __attribute__((packed))
#define cpu_to_le16(x) ((__le16)(x))
#define cpu_to_le32(x) ((__le32)(x))
#define __struct_group(TAG,NAME,ATTRS,...) union{struct{__VA_ARGS__}ATTRS;struct TAG{__VA_ARGS__}ATTRS NAME;}ATTRS
#define QRTR_PORT_CTRL 0xfffffffe
'''
    c += re.search(r'struct glink_msg \{[\s\S]*?\} __packed;', glink)[0] + '\n'
    c += re.search(r'struct qrtr_hdr_v1 \{[\s\S]*?\} __packed;', qrtr)[0] + '\n'
    for name in ['QRTR_PROTO_VER_1']:
        c += re.search(r'^#define\s+' + name + r'\s+[^\n]+', qrtr, re.M)[0] + '\n'
    for name in ['GLINK_CMD_TX_DATA', 'GLINK_CMD_TX_DATA_CONT']:
        c += re.search(r'^#define\s+' + name + r'\s+[^\n]+', glink, re.M)[0] + '\n'
    c += 'int main(void){\n'
    c += 'static const u8 qmi[]={' + ','.join(str(b) for b in qmi) + '};\n'
    c += r'''
 u8 packet[140]={0},frame[160]={0};
 struct qrtr_hdr_v1 *hdr=(void*)packet;
 struct {unsigned sq_node,sq_port;} from_value={1,0x4004},to_value={0,27},*from=&from_value,*to=&to_value;
 struct {unsigned nid;} node_value={0},*node=&node_value;
 int type=1,len=sizeof(qmi);bool confirm_rx=true;
'''
    # Original header assignments, including the real control-port branch.
    c += re.search(r'\thdr->version = cpu_to_le32\(QRTR_PROTO_VER_1\);[\s\S]*?\thdr->confirm_rx = cpu_to_le32\(!!confirm_rx\);', qrtr)[0] + '\n'
    c += 'memcpy(packet+sizeof(*hdr),qmi,sizeof(qmi));len=sizeof(packet);\n'
    send = glink[glink.index('static int __qcom_glink_send('):]
    c += re.search(r'\tstruct \{\n\t\tstruct glink_msg_hdr msg;[^{}]*\} __packed req;', send)[0] + '\n'
    c += 'int iid=9,chunk_size=len;size_t offset=0;struct {int lcid;} value={1},*channel=&value;\n'
    c += re.search(r'\t\treq.msg.cmd = cpu_to_le16\(offset == 0[^\n]+;[\s\S]*?\t\treq.left_size = cpu_to_le32\(len - offset - chunk_size\);', glink)[0] + '\n'
    c += r'''
 _Static_assert(sizeof(req)==16,"GLINK data header");
 _Static_assert(sizeof(*hdr)==32,"QRTR v1 header");
 memcpy(frame,&req,sizeof(req));memcpy(frame+sizeof(req),packet,sizeof(packet));
 for(size_t i=0;i<sizeof(frame);i++){printf("%02x",frame[i]);}puts("");return 0;}
'''
    path = OUT / 'linux-wire.c'
    path.write_text(c)
    run([shutil.which('clang') or 'cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror',
         str(path), '-o', str(OUT / 'linux-wire')], 'c-wire-build')
    result = run([str(OUT / 'linux-wire')], 'c-wire')
    path = OUT / 'linux-wire.txt'
    path.write_text(result.stdout)
    return path


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    run(['python3', str(ROOT / 'platforms/sunfish/test-native-ipa-linux-qmi.py')], 'qmi-reference')
    env = dict(os.environ, SUNFISH_LINUX_QMI_LINK_C=str(wire_reference()))
    rustc = shutil.which('rustc')
    test = ROOT / 'tests/sunfish_ipa_linux_qmi_link_test.rs'
    binary = OUT / 'link-test'
    flags = [rustc, '--edition=2024', '-D', 'warnings', '--cfg', FEATURE]
    run(flags + ['--test', str(test), '-o', str(binary)], 'rust-build', env)
    result = run([str(binary), 'linuxlink_'], 'link-tests', env)
    groups = int(re.search(r'test result: ok\. (\d+) passed;', result.stdout)[1])
    run([str(binary)], 'feature-regression', env)
    legacy = ROOT / 'tests/sunfish_glink_qmi_test.rs'
    run([rustc, '--edition=2024', '-D', 'warnings', '--test', str(legacy), '-o', str(OUT / 'legacy-test')], 'legacy-build')
    run([str(OUT / 'legacy-test')], 'legacy-regression')
    declarations = legacy.read_text().split('use qcom_glink_qmi::', 1)[0]
    wrapper = OUT / 'core.rs'
    wrapper.write_text('#![no_std]\n' + rust_modules(declarations) + '\n' + '\n'.join(
        f'#[path="{ROOT}/rust/src/{name}.rs"] pub mod {name};' for name in [
            'sunfish_ipa_linux_config', 'sunfish_ipa_linux_memory', 'sunfish_ipa_linux_qmi']))
    run(flags + ['--crate-type=lib', '--target=aarch64-unknown-none', str(wrapper),
                 '-o', str(OUT / 'link-aarch64.rlib')], 'aarch64-core')
    parent = ROOT / 'rust/src/qcom_glink_qmi.rs'
    child = ROOT / 'rust/src/sunfish_ipa_linux_qmi_link.rs'
    original_parent = parent.read_text()
    original_child = child.read_text()
    mutants = {
        'single-ack-retirement': ('parent', 'if !tx.glink_done || !tx.qrtr_done {', 'if !tx.glink_done && !tx.qrtr_done {'),
        'ignore-response-peer': ('child', 'self.control.ipa_modem_endpoint() != Some(source)', 'false && self.control.ipa_modem_endpoint() != Some(source)'),
        'reuse-record-sequence': ('child', 'id.sequence <= last.sequence', 'id.sequence < last.sequence'),
        'cross-record-owner': ('child', 'last.owner != id.owner', 'false && last.owner != id.owner'),
        'cross-session-receipt': ('child', 'receipt.owner != self.owner', 'false && receipt.owner != self.owner'),
        'ignore-endpoint-loss': ('child', 'self.control.ipa_modem_endpoint() != Some(expected)', 'false && self.control.ipa_modem_endpoint() != Some(expected)'),
        'unavailable-host-service': ('child', 'if !self.control.accepts(Service::IpaServer) {', 'if false && !self.control.accepts(Service::IpaServer) {'),
    }
    for label, (which, before, after) in mutants.items():
        folder = OUT / label
        folder.mkdir(exist_ok=True)
        for name in ['qcom_glink_qmi.rs', 'sunfish_ipa_linux_qmi_link.rs']:
            text = original_parent if name == parent.name else original_child
            if (which == 'parent') == (name == parent.name):
                if text.count(before) != 1:
                    raise ValueError('Mutation not unique: ' + label)
                text = text.replace(before, after)
            (folder / name).write_text(text)
        harness = rust_modules(legacy.read_text()).replace(str(parent), str(folder / parent.name))
        additions = test.read_text().replace('include!("sunfish_glink_qmi_test.rs");', '')
        harness += '\n' + rust_modules(additions)
        path = folder / 'test.rs'
        path.write_text(harness)
        exe = folder / 'test'
        run(flags + ['--test', str(path), '-o', str(exe)], label + '-build', env)
        if run([str(exe), 'linuxlink_'], label, env, False).returncode == 0:
            raise ValueError('Mutation accepted: ' + label)
    sdk = ROOT / '.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
    native_env = dict(os.environ, SEL4_INCLUDE_DIRS=str(sdk / 'board/sunfish/debug/include'),
                      CARGO_TARGET_DIR=str(ROOT / '.cache/sunfish-native-wifi/private/preflight-target'),
                      SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH=str(ROOT / '.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025/private_boot_inputs.rs'))
    run(['cargo', '+nightly-2026-09-25', 'build', '--locked', '--offline', '--release', '--lib',
         '--manifest-path', 'rust/Cargo.toml', '--features',
         'sunfish-native-startup-ipa-receive,sunfish-linux-ipa-qmi', '--target',
         'rust/targets/aarch64-sel4-microkit-pixel4k.json', '-Z', 'build-std=core,alloc',
         '-Z', 'json-target-spec'], 'sel4-library', native_env)
    selected = [str(p.relative_to(ROOT)) for p in [parent, child, test, legacy]] + [
        'rust/src/qcom_qmi_router.rs', 'rust/src/qcom_qrtr_control.rs', 'rust/src/qcom_qmi.rs',
        'rust/src/qcom_modem_services.rs', 'rust/src/sunfish_ipa_linux_qmi.rs',
        'rust/src/sunfish_ipa_linux_memory.rs', 'rust/src/sunfish_ipa_linux_config.rs',
        'rust/src/lib.rs', 'rust/Cargo.toml', 'rust/Cargo.lock',
        'platforms/sunfish/test-native-ipa-linux-qmi-link.py']
    selected += [str((ROOT / 'tests' / path).resolve().relative_to(ROOT))
                 for path in re.findall(r'#\[path = "([^"]+)"\]', declarations)]
    selected = sorted(set(selected))
    for name in selected:
        path = OUT / 'source' / name
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, path)
    manifest = {'kernel_commit': PIN, 'archive_sha256': ARCHIVE_HASH,
                'source_sha256': {name: sha(ROOT / name) for name in selected},
                'reference_sha256': {name: sha(OUT / 'reference' / name) for name in REFERENCE},
                'linux_link_groups': groups, 'legacy_regression_groups': 133,
                'aarch64_no_std': True, 'full_sel4_library': True,
                'mutations_rejected': list(mutants), 'device_actions': [],
                'scope': 'Actual bounded GLINK/QRTR/router plus Linux QMI profile under host record publication stand-ins; no native FIFO/IRQ attachment, hardware readiness, MPSS boot or Wi-Fi proof.'}
    manifest['artifacts'] = {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p)}
                             for p in OUT.iterdir() if p.is_file() and p.name != 'manifest.json'}
    manifest['mutation_artifacts'] = {str(p.relative_to(OUT)): {'bytes': p.stat().st_size, 'sha256': sha(p)}
                                      for label in mutants for p in (OUT / label).iterdir() if p.is_file()}
    qmi = ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi'
    proof = OUT / 'qmi-reference'
    proof.mkdir(exist_ok=True)
    for name in ['manifest.json', 'linux-qmi.txt', 'linux-qmi-oracle.c']:
        shutil.copyfile(qmi / name, proof / name)
    manifest['qmi_reference'] = {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p)} for p in proof.iterdir() if p.is_file()}
    (OUT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'PASS: {groups} Linux IPA link groups, 133 legacy regressions with and without profile support, {len(mutants)} rejected mutations, AArch64 no_std and full seL4 library; no device action.')


if __name__ == '__main__':
    main()
