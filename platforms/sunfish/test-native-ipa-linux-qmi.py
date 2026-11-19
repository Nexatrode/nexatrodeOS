#!/usr/bin/env python3
"""Compare the native IPA QMI profile with the pinned Linux factory/encoder.

No phone access. The oracle compiles original Linux message descriptors,
encoder, resource table, request factory and handshake callbacks. Only kernel
allocation, transport and object lookup are replaced by host stand-ins.
"""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi'
PIN = '2267e248204e7e051e0f7355298949a6440de579'
ARCHIVE_HASH = 'd55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'
SOURCES = [
    'drivers/net/ipa/ipa_qmi.c', 'drivers/net/ipa/ipa_qmi_msg.c',
    'drivers/net/ipa/ipa_qmi_msg.h', 'drivers/net/ipa/ipa_mem.h',
    'drivers/net/ipa/ipa_version.h', 'drivers/net/ipa/data/ipa_data-v4.2.c',
    'drivers/soc/qcom/qmi_encdec.c', 'drivers/soc/qcom/qmi_interface.c',
    'include/linux/soc/qcom/qmi.h',
]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(argv, label, env=None, success=True):
    result = subprocess.run(argv, cwd=ROOT, env=env, capture_output=True, text=True)
    (OUT / (label + '.log')).write_text(result.stdout + result.stderr)
    if success and result.returncode:
        raise SystemExit(f'{label}: exit {result.returncode}; see {OUT / (label + ".log")}')
    return result


def block(text, start):
    """Retain a complete original declaration/function, without rewriting it."""
    at = text.index(start)
    opening = text.index('{', at)
    end, depth = opening + 1, 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    if text[end:end + 1] == ';':
        end += 1
    return text[at:end] + '\n'


def no_includes(text):
    return re.sub(r'^#include[^\n]*\n', '', text, flags=re.M)


def oracle():
    archive = ROOT / '.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive) != ARCHIVE_HASH:
        raise ValueError('Wrong pinned kernel archive')
    originals = {}
    with tarfile.open(archive) as tar:
        members = {m.name.split('/', 1)[1]: m for m in tar.getmembers() if '/' in m.name}
        for name in SOURCES:
            text = tar.extractfile(members[name]).read().decode()
            originals[name] = text
            path = OUT / 'linux-source' / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
    qmi = originals['drivers/net/ipa/ipa_qmi.c']
    data = originals['drivers/net/ipa/data/ipa_data-v4.2.c']
    header = originals['include/linux/soc/qcom/qmi.h']
    c = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <sys/types.h>
typedef uint8_t u8; typedef uint16_t u16; typedef uint32_t u32; typedef uint64_t u64;
typedef int16_t s16; typedef int32_t s32;
typedef u16 __le16; typedef u32 __le32; typedef u64 __le64;
#define __packed __attribute__((packed))
#define __cpu_to_le16(x) ((u16)(x))
#define __cpu_to_le32(x) ((u32)(x))
#define __cpu_to_le64(x) ((u64)(x))
#define __le16_to_cpu(x) ((u16)(x))
#define __le32_to_cpu(x) ((u32)(x))
#define __le64_to_cpu(x) ((u64)(x))
#define cpu_to_le16 __cpu_to_le16
#define ARRAY_SIZE(a) (sizeof(a)/sizeof((a)[0]))
#define sizeof_field(t,f) sizeof(((t*)0)->f)
#define container_of(p,t,f) ((t*)((char*)(p)-offsetof(t,f)))
#define GFP_KERNEL 0
#define kzalloc(n,f) calloc(1,n)
#define kfree free
#define ERR_PTR(n) ((void*)(intptr_t)(n))
#define PTR_ERR(p) ((intptr_t)(p))
#define IS_ERR(p) ((uintptr_t)(p)>=(uintptr_t)-4095)
#define ETOOSMALL 525
#define U8_MAX UINT8_MAX
#define U16_MAX UINT16_MAX
#define mutex_init(p) ((void)(p))
#define mutex_lock(p) ((void)(p))
#define mutex_unlock(p) ((void)(p))
#define init_completion(p) ((void)(p))
#define pr_err(...) ((void)0)
#define dev_err(dev,...) ((void)(dev))
#define EXPORT_SYMBOL_GPL(x)
#define MODULE_DESCRIPTION(x)
#define MODULE_LICENSE(x)
'''
    # Types and descriptor enums are original. Unused kernel handle declarations
    # after this boundary are replaced below by deterministic host objects.
    c += no_includes(header[:header.index('/**\n * struct qmi_service')]) + '\n#endif\n'
    c += no_includes(originals['drivers/net/ipa/ipa_qmi_msg.h'])
    c += no_includes(originals['drivers/soc/qcom/qmi_encdec.c'])
    c += no_includes(originals['drivers/net/ipa/ipa_qmi_msg.c'])
    c += block(originals['drivers/net/ipa/ipa_mem.h'], 'enum ipa_mem_id {')
    c += block(originals['drivers/net/ipa/ipa_mem.h'], 'struct ipa_mem {')
    c += block(originals['drivers/net/ipa/ipa_version.h'], 'enum ipa_version {')
    c += block(data, 'static const struct ipa_mem ipa_mem_local_data[] = {')
    c += r'''
enum {IPA_ENDPOINT_AP_MODEM_RX};
struct ipa_endpoint {u32 endpoint_id;};
struct qmi_handle {int sock_lock,txn_lock,txns;void *sock;};
struct sockaddr_qrtr {u32 sq_family, sq_node, sq_port;};
struct qmi_txn {u32 id;int lock,completion;struct qmi_handle *qmi;
 const struct qmi_elem_info *ei;void *dest;};
struct ipa_qmi {struct qmi_handle server_handle; struct sockaddr_qrtr modem_sq;
 bool initial_boot, modem_ready, uc_ready, indication_requested, indication_sent;};
struct ipa {struct ipa_qmi qmi; bool uc_loaded; u32 modem_route_count, mem_offset;
 enum ipa_version version; struct ipa_endpoint *name_map[1]; void *dev;};
static const struct ipa_mem *ipa_mem_find(struct ipa *ipa,enum ipa_mem_id id){
 (void)ipa;for(size_t n=0;n<ARRAY_SIZE(ipa_mem_local_data);n++){
 if(ipa_mem_local_data[n].id==id)return &ipa_mem_local_data[n];}return NULL;}
static int response_error, indication_error, allocation_id=0x2468;
static u8 last_sent[256];static size_t last_sent_bytes;
struct msghdr {void *msg_name;size_t msg_namelen;};
struct kvec {void *iov_base;size_t iov_len;};
static int idr_alloc_cyclic(int *ids,struct qmi_txn *t,int low,int high,int flags){
 (void)ids;(void)t;(void)low;(void)high;(void)flags;return allocation_id;}
static void qmi_txn_cancel(struct qmi_txn *t){(void)t;}
static int kernel_sendmsg(void *sock,struct msghdr *m,struct kvec *v,int count,size_t len){
 (void)sock;(void)m;if(count!=1||v->iov_len!=len||len>sizeof(last_sent))abort();
 int error=((u8*)v->iov_base)[0]==QMI_RESPONSE?response_error:indication_error;
 if(error)return error;memcpy(last_sent,v->iov_base,len);last_sent_bytes=len;return (int)len;}
static void emit(const char *name,int type,u32 id,u32 txn,size_t len,const struct qmi_elem_info *ei,const void *value){
 void *bytes=qmi_encode_message(type,id,&len,txn,ei,value);if(IS_ERR(bytes))abort();
 printf("%s ",name);for(size_t i=0;i<len;i++)printf("%02x",((u8*)bytes)[i]);puts("");free(bytes);}
static int ipa_modem_start(struct ipa *ipa){(void)ipa;return 0;}
'''
    interface = originals['drivers/soc/qcom/qmi_interface.c']
    for start in ['int qmi_txn_init(', 'static ssize_t qmi_send_message(',
                  'ssize_t qmi_send_request(', 'ssize_t qmi_send_response(',
                  'ssize_t qmi_send_indication(']:
        c += block(interface, start)
    c += block(qmi, 'static const struct ipa_init_modem_driver_req *\ninit_modem_driver_req(')
    for function in ['ipa_server_init_complete', 'ipa_qmi_indication', 'ipa_qmi_ready',
                     'ipa_server_bye', 'ipa_server_indication_register',
                     'ipa_server_driver_init_complete']:
        c += block(qmi, 'static void ' + function + '(')
    endpoint = block(data, '[IPA_ENDPOINT_AP_MODEM_RX] = {')
    endpoint_id = re.search(r'\.endpoint_id\s*=\s*([^,]+),', endpoint)[1]
    route_count = re.search(r'\.modem_route_count\s*=\s*([^,]+),', data)[1]
    c += f'\n#define SOURCE_ENDPOINT_ID {endpoint_id}\n#define SOURCE_MODEM_ROUTES {route_count}\n'
    c += r'''
int main(void){
 struct ipa_endpoint ep={.endpoint_id=SOURCE_ENDPOINT_ID};
 struct ipa ipa={.modem_route_count=SOURCE_MODEM_ROUTES,.version=IPA_VERSION_4_2,.name_map={&ep}};
 const struct ipa_init_modem_driver_req *req=init_modem_driver_req(&ipa.qmi);
 emit("cold",QMI_REQUEST,IPA_QMI_INIT_DRIVER,0x1234,IPA_QMI_INIT_DRIVER_REQ_SZ,ipa_init_modem_driver_req_ei,req);
 ipa.uc_loaded=true;req=init_modem_driver_req(&ipa.qmi);
 emit("loaded",QMI_REQUEST,IPA_QMI_INIT_DRIVER,0x1234,IPA_QMI_INIT_DRIVER_REQ_SZ,ipa_init_modem_driver_req_ei,req);
 struct ipa_indication_register_rsp reg={0};struct ipa_driver_init_complete_rsp done={0};struct ipa_init_complete_ind ind={0};
 emit("register_response",QMI_RESPONSE,IPA_QMI_INDICATION_REGISTER,0x5678,IPA_QMI_INDICATION_REGISTER_RSP_SZ,ipa_indication_register_rsp_ei,&reg);
 emit("driver_response",QMI_RESPONSE,IPA_QMI_DRIVER_INIT_COMPLETE,0x9abc,IPA_QMI_DRIVER_INIT_COMPLETE_RSP_SZ,ipa_driver_init_complete_rsp_ei,&done);
 struct qmi_handle h={.sock=&h};struct sockaddr_qrtr helper_sq={0};
 for(int i=0;i<2;i++){
  allocation_id=i?0:0x2468;
  if(qmi_send_indication(&h,&helper_sq,IPA_QMI_INIT_COMPLETE,IPA_QMI_INIT_COMPLETE_IND_SZ,ipa_init_complete_ind_ei,&ind))abort();
  printf("%s ",i?"complete_indication_zero":"complete_indication");
  for(size_t j=0;j<last_sent_bytes;j++)printf("%02x",last_sent[j]);puts("");
 }
 const char *orders[]={"IRU","IUR","RIU","RUI","UIR","URI"};
 for(size_t n=0;n<ARRAY_SIZE(orders);n++){
  struct ipa p={.qmi={.initial_boot=true}};p.qmi.server_handle.sock=&p;
  struct sockaddr_qrtr sq={0};struct qmi_txn txn={0};
  for(size_t i=0;i<3;i++){
   switch(orders[n][i]){
    case 'I':p.qmi.modem_ready=true;ipa_qmi_ready(&p.qmi);break;
    case 'R':ipa_server_indication_register(&p.qmi.server_handle,&sq,&txn,NULL);break;
    case 'U':ipa_server_driver_init_complete(&p.qmi.server_handle,&sq,&txn,NULL);break;}
   printf("trace %s %zu %d %d %d %d %d\n",orders[n],i,p.qmi.modem_ready,p.qmi.uc_ready,p.qmi.indication_requested,p.qmi.indication_sent,p.qmi.initial_boot);
  }
 }
 struct ipa p={.qmi={.initial_boot=true,.modem_ready=true}};p.qmi.server_handle.sock=&p;
 struct sockaddr_qrtr sq={0};struct qmi_txn txn={0};
 response_error=-EIO;ipa_server_driver_init_complete(&p.qmi.server_handle,&sq,&txn,NULL);
 ipa_server_indication_register(&p.qmi.server_handle,&sq,&txn,NULL);
 printf("response_failed %d %d %d\n",p.qmi.uc_ready,p.qmi.indication_requested,p.qmi.indication_sent);
 response_error=0;indication_error=-EIO;
 ipa_server_driver_init_complete(&p.qmi.server_handle,&sq,&txn,NULL);
 ipa_server_indication_register(&p.qmi.server_handle,&sq,&txn,NULL);
 printf("indication_failed %d %d %d %d\n",p.qmi.uc_ready,p.qmi.indication_requested,p.qmi.indication_sent,p.qmi.initial_boot);
 ipa_server_bye(&p.qmi.server_handle,0);
 printf("bye %d %d %d %d %d\n",p.qmi.modem_ready,p.qmi.uc_ready,p.qmi.indication_requested,p.qmi.indication_sent,p.qmi.initial_boot);
 return 0;}
'''
    path = OUT / 'linux-qmi-oracle.c'
    path.write_text(c)
    run([shutil.which('clang') or 'cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror',
         '-Wno-unused-function', '-Wno-unused-parameter', '-Wno-sign-compare',
         str(path), '-o', str(OUT / 'linux-qmi-oracle')], 'c-build')
    golden = OUT / 'linux-qmi.txt'
    golden.write_text(run([str(OUT / 'linux-qmi-oracle')], 'c-oracle').stdout)
    return golden


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    golden = oracle()
    env = dict(os.environ, SUNFISH_LINUX_QMI_C=str(golden))
    rustc = shutil.which('rustc')
    test = ROOT / 'tests/sunfish_ipa_linux_qmi_test.rs'
    binary = OUT / 'qmi-test'
    run([rustc, '--edition=2024', '-D', 'warnings', '--test', str(test), '-o', str(binary)], 'rust-build', env)
    result = run([str(binary)], 'rust-tests', env)
    groups = int(re.search(r'test result: ok\. (\d+) passed;', result.stdout)[1])
    core_modules = ['qcom_qmi', 'sunfish_ipa_linux_config',
                    'sunfish_ipa_linux_memory', 'sunfish_ipa_linux_qmi']
    wrapper = OUT / 'core.rs'
    wrapper.write_text('#![no_std]\n' + '\n'.join(
        f'#[path="{ROOT}/rust/src/{name}.rs"] pub mod {name};' for name in core_modules))
    run([rustc, '--edition=2024', '-D', 'warnings', '--crate-type=lib',
         '--target=aarch64-unknown-none', str(wrapper), '-o', str(OUT / 'qmi-aarch64.rlib')],
        'aarch64-core')
    original = (ROOT / 'rust/src/sunfish_ipa_linux_qmi.rs').read_text()
    mutants = {
        'route-count-as-end-index': ('pair(r.offset as u32, MODEM_ROUTE_COUNT - 1)',
                                     'pair(r.offset as u32, MODEM_ROUTE_COUNT)'),
        'skip-cold-uc-load': ('append(0x18, &[u8::from(hardware_uc_loaded)]);',
                             'let _ = hardware_uc_loaded; append(0x18, &[1]);'),
        'ready-before-publication': ('let operation = server_operation(&m)?;',
                                     'let operation = server_operation(&m)?; if matches!(operation, Operation::DriverComplete { .. }) { self.state.qmi_uc_ready = true; }'),
        'wrong-modem-peer': ('if peer != self.modem {', 'if false && peer != self.modem {'),
        'ignore-remote-error': ('if result != 0 || error != 0 {',
                                'if false && (result != 0 || error != 0) {'),
        'cross-owner-publication': ('if prepared.owner != self.owner || prepared.generation != self.generation {',
                                    'if prepared.generation != self.generation {'),
        'hardcoded-indication-id': ('common(4, transaction, INIT_COMPLETE)',
                                    '{ let _ = transaction; common(4, 0, INIT_COMPLETE) }'),
    }
    for label, (before, after) in mutants.items():
        if original.count(before) != 1:
            raise ValueError('Mutation not unique: ' + label)
        mutated = OUT / (label + '.rs')
        mutated.write_text(original.replace(before, after))
        harness = test.read_text()
        for name in core_modules:
            target = mutated if name == 'sunfish_ipa_linux_qmi' else ROOT / 'rust/src' / (name + '.rs')
            harness = harness.replace('../rust/src/' + name + '.rs', str(target))
        path = OUT / (label + '-test.rs')
        path.write_text(harness)
        exe = OUT / (label + '-test')
        run([rustc, '--edition=2024', '-D', 'warnings', '--test', str(path), '-o', str(exe)],
            label + '-build', env)
        if run([str(exe)], label, env, False).returncode == 0:
            raise ValueError('Mutation accepted: ' + label)
    selected = ['rust/src/sunfish_ipa_linux_qmi.rs', 'rust/src/sunfish_ipa_linux_memory.rs',
                'rust/src/sunfish_ipa_linux_config.rs', 'rust/src/qcom_qmi.rs',
                'rust/src/lib.rs', 'tests/sunfish_ipa_linux_qmi_test.rs',
                'platforms/sunfish/test-native-ipa-linux-qmi.py']
    for name in selected:
        path = OUT / 'source' / name
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, path)
    manifest = {'kernel_commit': PIN, 'archive_sha256': ARCHIVE_HASH,
                'source_sha256': {n: sha(ROOT / n) for n in selected},
                'original_linux_sources': {n: sha(OUT / 'linux-source' / n) for n in SOURCES},
                'behavioral_groups': groups, 'mutations_rejected': list(mutants),
                'aarch64_no_std': True,
                'library_exported': 'pub mod sunfish_ipa_linux_qmi;' in (ROOT / 'rust/src/lib.rs').read_text(),
                'device_actions': [],
                'scope': 'Offline exact IPA QMI bytes and protocol state; no native transport binding, modem startup, hardware readiness or Wi-Fi proof.'}
    manifest['artifacts'] = {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p)}
                             for p in OUT.iterdir() if p.is_file() and p.name != 'manifest.json'}
    (OUT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'PASS: {groups} IPA QMI groups against original Linux, {len(mutants)} rejected mutations and AArch64 no_std; no device action.')


if __name__ == '__main__':
    main()
