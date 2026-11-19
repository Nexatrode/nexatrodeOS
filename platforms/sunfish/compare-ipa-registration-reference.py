#!/usr/bin/env python3
"""Compile original Linux/Android registration callbacks with recording stubs.

Tests callback ordering, not Linux scheduling or modem behavior. No device I/O.
"""
from pathlib import Path
import argparse
import hashlib
import json
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--output', type=Path, required=True)
a = p.parse_args()
out = a.output.resolve()
out.mkdir(parents=True, exist_ok=True, mode=0o700)
linux = ROOT / '.cache/sunfish-mobian-comparison/source-review/kernel/drivers/net/ipa/ipa_qmi.c'
android = ROOT / '.cache/sunfish-lineage-kernel/drivers/platform/msm/ipa/ipa_v3/ipa_qmi_service.c'

def function(path, name):
    text = path.read_text()
    start = text.index('static void ' + name + '(')
    body = text.index('{', start)
    depth = 1
    end = body + 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    return text[start:end] + '\n'

c = r'''
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
struct qmi_handle { int unused; };
struct sockaddr_qrtr { unsigned family, node, port; };
struct qmi_txn { int id; };
struct result { uint16_t result, error; };
struct device { int unused; };
struct ipa_indication_register_rsp { struct result rsp; };
struct ipa_qmi { struct qmi_handle server_handle; bool modem_ready, indication_requested; };
struct ipa { struct ipa_qmi qmi; struct device *dev; };
#define container_of(p,t,m) ((t *)((char *)(p) - offsetof(t,m)))
#define dev_err(...) ((void)0)
#define IPA_QMI_INDICATION_REGISTER 0x20
#define IPA_QMI_INDICATION_REGISTER_RSP_SZ 14
#define QMI_RESULT_SUCCESS_V01 0
#define QMI_ERR_NONE_V01 0
#define ipa_indication_register_rsp_ei NULL
struct ipa_indication_reg_req_msg_v01 { int unused; };
struct ipa_indication_reg_resp_msg_v01 { struct result resp; };
struct ipa_master_driver_init_complt_ind_msg_v01 { struct result master_driver_init_status; };
static struct { struct sockaddr_qrtr client_sq; } ctx, *ipa3_qmi_ctx = &ctx;
static bool ipa3_qmi_indication_fin, ipa3_qmi_modem_init_fin;
#define IPAWANDBG(...) ((void)0)
#define IPAWANERR(...) ((void)0)
#define IPA_QMI_RESULT_SUCCESS_V01 0
#define QMI_IPA_INDICATION_REGISTER_RESP_V01 0x20
#define QMI_IPA_INDICATION_REGISTER_RESP_MAX_MSG_LEN_V01 14
#define QMI_IPA_MASTER_DRIVER_INIT_COMPLETE_IND_V01 0x22
#define QMI_IPA_MASTER_DRIVER_INIT_COMPLETE_IND_MAX_MSG_LEN_V01 14
#define ipa3_indication_reg_resp_msg_data_v01_ei NULL
#define ipa3_master_driver_init_complt_ind_msg_data_v01_ei NULL
static int responses, indications, readiness_checks, failure;
static int qmi_send_response(struct qmi_handle *h, struct sockaddr_qrtr *sq,
 struct qmi_txn *txn, int id, int size, const void *ei, void *rsp) {
 (void)h;(void)sq;(void)txn;(void)size;(void)ei;
 assert(id == 0x20);assert(((struct result *)rsp)->result == 0);
 responses++;return failure ? -1 : 0;
}
static int qmi_send_indication(struct qmi_handle *h, struct sockaddr_qrtr *sq,
 int id, int size, const void *ei, void *ind) {
 (void)h;(void)sq;(void)size;(void)ei;(void)ind;
 assert(id == 0x22);assert(responses == 1);indications++;return 0;
}
static void ipa_qmi_ready(struct ipa_qmi *qmi) {
 assert(responses == 1 && qmi->indication_requested);readiness_checks++;
}
'''
c += function(linux, 'ipa_server_indication_register')
c += function(android, 'ipa3_handle_indication_req')
c += r'''
int main(void) {
 for (int ready = 0; ready < 2; ready++) for (int failed = 0; failed < 2; failed++) {
  struct ipa ipa = {.qmi = {.modem_ready = ready}};
  struct sockaddr_qrtr sq = {.node = 3, .port = 29};
  struct qmi_txn txn = {.id = 17};struct qmi_handle h = {0};
  responses=indications=readiness_checks=0;failure=failed;
  ipa_server_indication_register(&ipa.qmi.server_handle,&sq,&txn,NULL);
  assert(responses==1 && ipa.qmi.indication_requested==!failed);
  assert(readiness_checks==!failed);
  printf("linux ready=%d send_failed=%d response_calls=%d registered=%d\n",ready,failed,responses,ipa.qmi.indication_requested);
  responses=indications=0;ipa3_qmi_modem_init_fin=ready;ipa3_qmi_indication_fin=false;
  ipa3_handle_indication_req(&h,&sq,&txn,NULL);
  assert(responses==1 && ipa3_qmi_indication_fin==!failed);
  assert(indications==(ready && !failed));
  assert(ctx.client_sq.node==sq.node && ctx.client_sq.port==sq.port);
  printf("android ready=%d send_failed=%d response_calls=%d registered=%d indications=%d\n",ready,failed,responses,ipa3_qmi_indication_fin,indications);
 }
}
'''
source = out / 'registration-reference.c'
source.write_text(c)
command = [shutil.which('clang'), '-std=c11', '-Wall', '-Wextra', '-Werror',
           '-Wno-unused-variable', '-Wno-unused-but-set-variable', '-Wno-unused-parameter',
           str(source), '-o', str(out / 'registration-reference')]
subprocess.run(command, check=True)
r = subprocess.run([str(out / 'registration-reference')], capture_output=True, text=True, check=True)
(out / 'registration-reference.log').write_text(r.stdout + r.stderr)
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
(out / 'registration-reference.json').write_text(json.dumps(dict(
    source_sha256={str(p.relative_to(ROOT)): sha(p) for p in [linux, android]},
    extracted_callback_harness_sha256=sha(source), compiler_command=command,
    result='passed', cases=8, device_actions=[],
    limitation='Records qmi_send_response calls; kernel transport and readiness helper are stubs. Does not prove concurrency or modem acceptance.'), indent=2)+'\n')
print(r.stdout)
