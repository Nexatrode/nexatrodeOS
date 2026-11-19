#!/usr/bin/env python3
"""Run exact Linux/Android GLINK send functions with recording transport stubs.

An already-active intent request is modeled by a request stub that returns
EBUSY. This tests the ready-intent path, not kernel threads or modem behavior.
"""
from pathlib import Path
import argparse, hashlib, json, shutil, subprocess
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
out=a.output.resolve();out.mkdir(parents=True,exist_ok=True,mode=0o700)
files={'linux':ROOT/'.cache/sunfish-mobian-comparison/source-review/kernel/drivers/rpmsg/qcom_glink_native.c',
       'android':ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg/qcom_glink_native.c'}
stubs=r'''
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <errno.h>
typedef uint32_t __le32;
#define __packed __attribute__((packed))
#define cpu_to_le16(x) (x)
#define cpu_to_le32(x) (x)
#define SZ_8K 8192
#define GLINK_CMD_TX_DATA 9
#define GLINK_CMD_TX_DATA_CONT 12
#define RPM_CMD_TX_DATA 9
#define RPM_CMD_TX_DATA_CONT 12
#define spin_lock_irqsave(lock, flags) ((flags)=0)
#define spin_unlock_irqrestore(lock, flags) ((void)(flags))
#define atomic_read(p) (*(p))
#define trace_qcom_glink_cmd_tx_data_tx(...) ((void)0)
struct glink_msg_hdr {uint16_t cmd,param1; uint32_t param2;};
struct glink_msg {uint16_t cmd,param1; uint32_t param2;};
struct glink_core_rx_intent {size_t size;bool in_use;int id;};
struct idr {struct glink_core_rx_intent *slots[2];};
#define idr_for_each_entry(map,tmp,iid) for((iid)=0;(iid)<2;(iid)++) if(((tmp)=(map)->slots[iid])!=NULL)
struct qcom_glink {bool intentless;int in_reset;};
struct glink_channel {struct qcom_glink *glink;struct idr riids;int lcid;};
static int request_calls,tx_calls,selected_id;
static int qcom_glink_request_intent(struct qcom_glink *g,struct glink_channel *c,int len) {
 (void)g;(void)c;(void)len;request_calls++;return -EBUSY;
}
static int qcom_glink_tx(struct qcom_glink *g,void *header,size_t header_size,const void *data,size_t len,bool wait) {
 (void)g;(void)data;(void)len;(void)wait;assert(header_size==16);
 struct glink_msg_hdr *h=header;assert(h->cmd==9);selected_id=h->param2;tx_calls++;return 0;
}
'''
main=r'''
int main(void) {
 int lengths[]={48,140,400};int cases=0;
 for(int busy=0;busy<2;busy++)for(int wait=0;wait<2;wait++)for(int i=0;i<3;i++) {
  struct qcom_glink g={0};
  struct glink_core_rx_intent small={64,busy,7},large={256,busy,8};
  struct glink_channel c={.glink=&g,.riids={{&small,&large}},.lcid=1};
  char data[400]={0};request_calls=tx_calls=selected_id=0;
  int len=lengths[i],ret=__qcom_glink_send(&c,data,len,wait);
  bool fits=!busy && len<=256;
  assert(ret==(fits ? 0 : -EBUSY));assert(tx_calls==fits);
  assert(request_calls==(!fits && wait));
  if(fits)assert(selected_id==(len<=64 ? 7 : 8));
  printf("length=%d busy=%d wait=%d sent=%d request_calls=%d\n",len,busy,wait,tx_calls,request_calls);cases++;
 }
 assert(cases==12);
}
'''
records={}
for mode,path in files.items():
 text=path.read_text();start=text.index('static int __qcom_glink_send(');body=text.index('{',start);depth=1;end=body+1
 while depth:
  depth+=(text[end]=='{')-(text[end]=='}');end+=1
 callback=text[start:end]+'\n';source=out/(mode+'-send.c');source.write_text(stubs+callback+main)
 command=[shutil.which('clang'),'-std=gnu11','-Wall','-Wextra','-Werror','-Wno-sign-compare',str(source),'-o',str(out/(mode+'-send'))]
 compile=subprocess.run(command,capture_output=True,text=True);(out/(mode+'-build.log')).write_text(compile.stdout+compile.stderr)
 if compile.returncode:raise SystemExit(compile.stderr)
 r=subprocess.run([str(out/(mode+'-send'))],capture_output=True,text=True,check=True);(out/(mode+'-send.log')).write_text(r.stdout+r.stderr)
 sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
 records[mode]=dict(source_path=str(path),source_sha256=sha(path),function_sha256=hashlib.sha256(callback.encode()).hexdigest(),harness_sha256=sha(source),compiler_command=command,cases=12)
(out/'result.json').write_text(json.dumps(dict(result='passed',references=records,cases=24,device_actions=[],scope='Original send functions; ready-intent selection bypasses the request stub. Locks, allocation wait and transmission are stubs; no kernel concurrency or firmware result.'),indent=2)+'\n')
print('PASS: 24 original Linux/Android send cases; suitable free intents send without calling the busy request stub.')
