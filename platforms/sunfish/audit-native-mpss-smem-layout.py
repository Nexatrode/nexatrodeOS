#!/usr/bin/env python3
"""Compile pinned Linux allocation/IPA signaling to plan the composed boot.

CPU-only source audit. It changes no native authority or device state. The
selected order follows the native IPA-497 allocation with SMP2P and GLINK;
actual startup must validate/lock the live directory before using this plan.
"""
from pathlib import Path
import hashlib
import json
import os
import re
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT/'.cache/sunfish-native-wifi/private/mpss-smem-layout'
PIN = '2267e248204e7e051e0f7355298949a6440de579'
ARCHIVE_HASH = 'd55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'


def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()


def block(s, start):
    a=s.index(start); b=s.index('{',a)+1; depth=1
    while depth:
        depth+=(s[b]=='{')-(s[b]=='}');b+=1
    if s[b:b+1]==';':b+=1
    return s[a:b]+'\n'


def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    if OUT.stat().st_mode&0o077:raise ValueError('Private output required')
    archive=ROOT/'.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive)!=ARCHIVE_HASH:raise ValueError('Wrong Linux archive')
    names=['drivers/soc/qcom/smem.c','drivers/soc/qcom/smp2p.c','drivers/rpmsg/qcom_glink_smem.c',
           'drivers/net/ipa/ipa_smp2p.c','arch/arm64/boot/dts/qcom/sm7150.dtsi']
    source={}
    with tarfile.open(archive) as t:
        for name in names:
            data=t.extractfile(f'linux-{PIN}/{name}').read();source[name]=data.decode()
            p=OUT/'linux-source'/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
    smem,smp,glink,ipa,dt=(source[n] for n in names)
    node=block(dt,'smp2p-modem {')
    for token in ['qcom,smem = <435>, <428>;','qcom,local-pid = <0>;','qcom,remote-pid = <1>;',
                  'mboxes = <&apss_shared 14>;','GIC_SPI 451 IRQ_TYPE_EDGE_RISING',
                  'qcom,entry-name = "master-kernel";','qcom,entry-name = "ipa";']:
        assert token in node,token
    remote=block(dt,'remoteproc_mpss: remoteproc@4080000 {')
    assert '<GIC_SPI 449 IRQ_TYPE_EDGE_RISING>' in remote and 'mboxes = <&apss_shared 12>;' in remote
    assert '<&smp2p_modem_in 0 IRQ_TYPE_EDGE_RISING>' in remote
    assert '<&smp2p_modem_in 1 IRQ_TYPE_EDGE_RISING>' in remote
    assert '<&smp2p_modem_in 2 IRQ_TYPE_EDGE_RISING>' in remote
    for macro,value in [('SMEM_GLINK_NATIVE_XPRT_DESCRIPTOR',478),('SMEM_GLINK_NATIVE_XPRT_FIFO_0',479),('SMEM_GLINK_NATIVE_XPRT_FIFO_1',480)]:
        assert int(re.search(r'^#define\s+'+macro+r'\s+(\d+)',glink,re.M)[1])==value
    probe=block(glink,'struct qcom_glink_smem *qcom_glink_smem_register(')
    assert 'SMEM_GLINK_NATIVE_XPRT_DESCRIPTOR, 32)' in probe
    assert 'SMEM_GLINK_NATIVE_XPRT_FIFO_0,\n\t\t\t      SZ_16K)' in probe
    for token in ['tx_pipe->tail = &descs[0];','tx_pipe->head = &descs[1];',
                  'rx_pipe->tail = &descs[2];','rx_pipe->head = &descs[3];']:
        assert token in probe,token
    c=r'''
#include <stdint.h>
#include <stdlib.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <assert.h>
#include <errno.h>
#include <stddef.h>
typedef uint8_t u8;typedef uint16_t u16;typedef uint32_t u32;typedef uint16_t __le16;typedef uint32_t __le32;typedef uint64_t phys_addr_t;
#define __iomem
#define __force
#define __packed __attribute__((packed))
#define le16_to_cpu(x) (x)
#define le32_to_cpu(x) (x)
#define cpu_to_le16(x) (x)
#define cpu_to_le32(x) (x)
#define le32_add_cpu(p,n) (*(p)+=(n))
#define ALIGN(x,n) (((x)+(n)-1)&~((n)-1))
#define WARN_ON(x) (x)
#define dev_err(d,...) ((void)(d))
#define wmb() __sync_synchronize()
#define SMEM_PRIVATE_CANARY 0xa5a5
#define SMP2P_MAX_ENTRY 16
#define SMP2P_MAX_ENTRY_NAME 16
#define BIT(n) (1U<<(n))
struct qcom_smem{void *dev;};
'''
    for sig in ['struct smem_partition_header {','struct smem_partition {','struct smem_private_entry {',
                'static void *\nphdr_to_last_uncached_entry(', 'static void *\nphdr_to_last_cached_entry(',
                'static struct smem_private_entry *\nphdr_to_first_uncached_entry(',
                'static struct smem_private_entry *\nuncached_entry_next(', 'static int qcom_smem_alloc_private(']:
        c+=block(smem,sig)
    # Keep the original packed layout (including its feature bitfield).
    c+=block(smp,'struct smp2p_smem_item {').rstrip()+' __packed;\n'
    c+=r'''
_Static_assert(sizeof(struct smp2p_smem_item)==340,"SMP2P item");
_Static_assert(sizeof(struct smem_private_entry)==16,"SMEM item header");
_Static_assert(sizeof(struct smem_partition_header)==32,"SMEM partition header");
_Static_assert(offsetof(struct smp2p_smem_item,entries)==20,"SMP2P entry offset");
struct ipa{void *dev;};
struct qcom_smem_state{u32 value;};
struct ipa_smp2p{struct ipa *ipa;struct qcom_smem_state *valid_state,*enabled_state;u32 valid_bit,enabled_bit;bool power_on,notified;};
static int power_active;static u32 update_mask[2],update_value[2],updated;
static int pm_runtime_get_if_active(void *d){(void)d;return power_active;}
static int qcom_smem_state_update_bits(struct qcom_smem_state *s,u32 mask,u32 value){
 assert(updated<2);update_mask[updated]=mask;s->value=(s->value&~mask)|(value&mask);update_value[updated++]=s->value;return 0;
}
'''
    c+=block(ipa,'static void ipa_smp2p_notify(')
    c+=r'''
static unsigned char bytes[0x7c000];
static void one(unsigned a,unsigned b,unsigned d,bool print){
 memset(bytes,0xee,sizeof(bytes));struct smem_partition_header *h=(void*)bytes;memset(h,0,sizeof(*h));
 h->size=sizeof(bytes);h->offset_free_uncached=32;h->offset_free_cached=sizeof(bytes);
 struct smem_partition p={.virt_base=bytes,.phys_base=0x8611d000,.cacheline=32,.size=sizeof(bytes)};
 struct qcom_smem smem={0};assert(qcom_smem_alloc_private(&smem,&p,497,8192)==0);memset(bytes+48,0xa5,8192);
 unsigned order[3]={a,b,d};
 for(unsigned n=0;n<3;n++){unsigned item=order[n];size_t size=item==428?sizeof(struct smp2p_smem_item):item==478?32:16384;
  assert(qcom_smem_alloc_private(&smem,&p,item,size)==0);
 }
 assert(h->offset_free_uncached==0x61d8);assert(h->offset_free_cached==sizeof(bytes));
 for(unsigned i=48;i<48+8192;i++)assert(bytes[i]==0xa5);
 if(print){
  puts("{\"items\":[");struct smem_private_entry *e=phdr_to_first_uncached_entry(h);unsigned i=0;
  while((void*)e<phdr_to_last_uncached_entry(h)){
   printf("%s{\"id\":%u,\"header_offset\":%zu,\"payload_offset\":%zu,\"bytes\":%u,\"padding\":%u}",i++?",\n":"",e->item,(unsigned char*)e-bytes,(unsigned char*)(e+1)-bytes,e->size-e->padding_data,e->padding_data);e=uncached_entry_next(e);
  }printf("],\"free_after\":%u,\"smp2p_bytes\":%zu,",h->offset_free_uncached,sizeof(struct smp2p_smem_item));
 }
 unsigned cursor=h->offset_free_uncached;assert(qcom_smem_alloc_private(&smem,&p,428,340)==-EEXIST);assert(h->offset_free_uncached==cursor);
}
int main(void){
 unsigned ids[3]={428,478,479};
 for(unsigned a=0;a<3;a++)for(unsigned b=0;b<3;b++)for(unsigned d=0;d<3;d++)if(a!=b&&a!=d&&b!=d)one(ids[a],ids[b],ids[d],false);
 one(428,478,479,true);
 for(int on=0;on<2;on++){
  struct ipa ipa={0};struct qcom_smem_state state={0};struct ipa_smp2p s={.ipa=&ipa,.valid_state=&state,.enabled_state=&state,.valid_bit=0,.enabled_bit=1};
  power_active=on;updated=0;ipa_smp2p_notify(&s);assert(updated==2&&s.notified);
  assert(update_mask[0]==2&&update_mask[1]==1);assert(update_value[0]==(on?2U:0U)&&update_value[1]==(on?3U:1U));
  ipa_smp2p_notify(&s);assert(updated==2);
 }puts("\"allocation_orders_checked\":6,\"ipa_notify_when_power_on\":[2,3],\"ipa_notify_when_power_off\":[0,1]}");
}
'''
    (OUT/'layout-oracle.c').write_text(c)
    for args,label in [(['cc','-std=gnu11','-O2','-Wall','-Wextra','-Werror',str(OUT/'layout-oracle.c'),'-o',str(OUT/'layout-oracle')],'build'),([str(OUT/'layout-oracle')],'run')]:
        r=subprocess.run(args,cwd=ROOT,capture_output=True,text=True);(OUT/(label+'.log')).write_text(r.stdout+r.stderr)
        if r.returncode:raise SystemExit(f'{label} failed; see {OUT/(label+".log")}')
    result=json.loads(r.stdout)
    cursor=32;expected=[]
    for item,size in [(497,8192),(428,340),(478,32),(479,16384)]:
        padding=(-size)%8
        expected.append({'id':item,'header_offset':cursor,'payload_offset':cursor+16,'bytes':size,'padding':padding})
        cursor+=16+size+padding
    assert result['items']==expected and result['free_after']==cursor
    records=[]
    for i in expected:
        record=dict(i);record['physical_address']=0x8611d000+i['payload_offset']
        record['smem_offset']=0x11d000+i['payload_offset'];records.append(record)
    result.update({'result':'passed','kernel_commit':PIN,'archive_sha256':ARCHIVE_HASH,
                   'source_sha256':{p:hashlib.sha256(s.encode()).hexdigest() for p,s in source.items()},
                   'items':records,'partition':{'physical':0x8611d000,'bytes':0x7c000,'initial_free':32},
                   'required_new_payload_pages':{'physical':0x8611f000,'pages':5},
                   'outbound_entries':['master-kernel','ipa'],'inbound_entries':['slave-kernel','ipa'],
                   'root_physical_irqs':{'glink':481,'smp2p':483},'doorbell_bits':{'glink':12,'smp2p':14},
                   'master_kernel_inbound_bits':{'fatal':0,'ready':1,'handover':2,'stop_ack':3,'shutdown_ack':7},
                   'ipa_inbound_bits':{'clock_query':0,'setup_ready':1},'ipa_outbound_bits':{'clock_valid':0,'clock_enabled':1},
                   'c_oracle_sha256':sha(OUT/'layout-oracle.c'),'c_stdout_sha256':sha(OUT/'run.log'),
                   'scope':'CPU-only source-derived layout/signaling plan; live locked validation, exact store whitelist, native IRQ and lifecycle integration remain.',
                   'device_actions':[],'native_mpss_started':False})
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print('PASS: original Linux allocator, six allocation orders, preserved IPA payload, two-entry SMP2P layout and IPA power-state reply ordering. No device action.')


if __name__=='__main__':main()
