#!/usr/bin/env python3
"""Compile selected original pinned Linux SMP2P AP effects, not a reimplementation."""
from pathlib import Path
import hashlib, importlib.util, json, os, subprocess, tarfile
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-smp2p-linux'
PIN='2267e248204e7e051e0f7355298949a6440de579'
ARCHIVE_HASH='d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
spec=importlib.util.spec_from_file_location('layout',Path(__file__).with_name('audit-native-mpss-smem-layout.py'))
layout=importlib.util.module_from_spec(spec);spec.loader.exec_module(layout)
def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    if OUT.stat().st_mode&0o077:raise ValueError('private output required')
    archive=ROOT/'.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive)!=ARCHIVE_HASH:raise ValueError('wrong pinned Linux archive')
    sources={}
    with tarfile.open(archive)as t:
        for name in ['drivers/soc/qcom/smp2p.c','drivers/net/ipa/ipa_smp2p.c','arch/arm64/boot/dts/qcom/sm7150.dtsi','drivers/mailbox/qcom-apcs-ipc-mailbox.c']:
            data=t.extractfile(f'linux-{PIN}/{name}').read();sources[name]=data.decode()
            p=OUT/'linux-source'/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
    smp,ipa,dt,apcs=sources.values()
    mailbox=layout.block(dt,'apss_shared: mailbox@17c00000 {')
    assert '"qcom,sm7150-apss-shared"' in mailbox and '"qcom,sdm845-apss-shared"' in mailbox
    assert 'reg = <0x0 0x17c00000 0x0 0x1000>;' in mailbox
    assert '{ .compatible = "qcom,sdm845-apss-shared", .data = &apps_shared_apcs_data }' in apcs
    assert '.offset = 12' in layout.block(apcs,'static const struct qcom_apcs_ipc_data apps_shared_apcs_data =')
    assert 'GIC_SPI 451 IRQ_TYPE_EDGE_RISING' in layout.block(dt,'smp2p-modem {')
    assert 'mboxes = <&apss_shared 14>;' in layout.block(dt,'smp2p-modem {')
    c=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>
#include <errno.h>
#include <stddef.h>
typedef uint8_t u8;typedef uint16_t u16;typedef uint32_t u32;
#define __packed __attribute__((packed))
#define __iomem
#define SMP2P_MAX_ENTRY 16
#define SMP2P_MAX_ENTRY_NAME 16
#define SMP2P_FEATURE_SSR_ACK 1
#define SMP2P_ALL_FEATURES 1
#define SMP2P_FLAGS_RESTART_DONE_BIT 0
#define SMP2P_FLAGS_RESTART_ACK_BIT 1
#define SMP2P_MAGIC 0x504d5324
#define SMP2P_INBOUND 0
#define SMP2P_OUTBOUND 1
#define MAX_VERSION 2
#define BIT(n) (1U<<(n))
#define IS_ERR(p) ((intptr_t)(p)<0)
#define PTR_ERR(p) ((intptr_t)(p))
#define dev_err(d,...) ((void)(d))
#define dev_err_probe(d,r,...) (r)
#define trace_smp2p_negotiate(d,f) ((void)(d),(void)(f))
#define trace_smp2p_ssr_ack(d) ((void)(d))
#define trace_smp2p_update_bits(e,o,v) ((void)(e),(void)(o),(void)(v))
#define spin_lock_irqsave(l,f) ((void)(l),(f)=0)
#define spin_unlock_irqrestore(l,f) ((void)(l),(void)(f))
#define readl(p) (*(p))
#define writel(v,p) (*(p)=(v))
#define wmb() __sync_synchronize()
#define list_for_each_entry(e,h,n) for((void)(h),(e)=NULL;(e)!=NULL;(e)=NULL)
struct device_node{};
'''
    c+=layout.block(smp,'struct smp2p_smem_item {').rstrip()+' __packed;\n'
    c+=r'''
_Static_assert(sizeof(struct smp2p_smem_item)==340,"wire size");
struct qcom_smp2p {void *dev,*mbox_chan,*ipc_regmap;unsigned ipc_offset,ipc_bit,smem_items[2],remote_pid,local_pid;
 struct smp2p_smem_item *in,*out;bool ssr_ack_enabled,ssr_ack,negotiation_done;int inbound;};
struct smp2p_entry{const char *name;u32 *value,last_value;void *state;struct qcom_smp2p *smp2p;int lock;};
struct qcom_smem_state_ops {int(*update_bits)(void*,u32,u32);};
static struct smp2p_smem_item local,remote,snapshots[16];static unsigned kicks;static int power_active;
static void snapshot(void){assert(kicks<16);memcpy(&snapshots[kicks++],&local,sizeof(local));}
static int mbox_send_message(void*c,void*d){(void)c;(void)d;snapshot();return 0;}
static void mbox_client_txdone(void*c,int r){(void)c;(void)r;}
static int regmap_write(void*r,unsigned o,u32 v){(void)r;(void)o;assert(v==BIT(14));snapshot();return 0;}
static int qcom_smem_alloc(unsigned p,unsigned id,size_t n){assert(p==1&&id==428&&n==340);return 0;}
static void*qcom_smem_get(unsigned p,unsigned id,size_t*n){assert(p==1);if(n)*n=340;return id==428?(void*)&local:(void*)&remote;}
static void strscpy(char*d,const char*s,size_t n){assert(strlen(s)<n);strcpy(d,s);}
static void*qcom_smem_state_register(struct device_node*n,const struct qcom_smem_state_ops*o,void*e){(void)n;(void)o;return e;}
'''
    for sig in ['static void qcom_smp2p_kick(','static bool qcom_smp2p_check_ssr(', 'static void qcom_smp2p_do_ssr_ack(',
                'static void qcom_smp2p_negotiate(','static int qcom_smp2p_in_version(', 'static int smp2p_update_bits(']:c+=layout.block(smp,sig)
    c+='static const struct qcom_smem_state_ops smp2p_state_ops={.update_bits=smp2p_update_bits};\n'
    for sig in ['static int qcom_smp2p_outbound_entry(','static int qcom_smp2p_alloc_outbound_item(']:c+=layout.block(smp,sig)
    c+=r'''
struct ipa{void*dev;};
struct ipa_smp2p{struct ipa*ipa;void*valid_state,*enabled_state;u32 valid_bit,enabled_bit;bool power_on,notified;};
static int pm_runtime_get_if_active(void*d){(void)d;return power_active;}
static int qcom_smem_state_update_bits(void*s,u32 m,u32 v){return smp2p_update_bits(s,m,v);}
'''
    c+=layout.block(ipa,'static void ipa_smp2p_notify(')
    c+=r'''
static void save(const char*dir,const char*name,const void*data,size_t size){char path[4096];assert(snprintf(path,sizeof(path),"%s/%s",dir,name)>0);FILE*f=fopen(path,"wb");assert(f);assert(fwrite(data,1,size,f)==size);assert(fclose(f)==0);}
static void initialize(struct qcom_smp2p*s,struct smp2p_entry*master,struct smp2p_entry*ip){
 memset(&local,0,sizeof(local));memset(&remote,0,sizeof(remote));kicks=0;
 *s=(struct qcom_smp2p){.ipc_bit=14,.smem_items={435,428},.local_pid=0,.remote_pid=1};
 *master=(struct smp2p_entry){.name="master-kernel",.smp2p=s};*ip=(struct smp2p_entry){.name="ipa",.smp2p=s};
 assert(qcom_smp2p_alloc_outbound_item(s)==0);assert(qcom_smp2p_outbound_entry(s,master,NULL)==0);assert(qcom_smp2p_outbound_entry(s,ip,NULL)==0);qcom_smp2p_kick(s);
}
int main(int argc,char**argv){assert(argc==2);const char*dir=argv[1];struct qcom_smp2p s;struct smp2p_entry master,ip;initialize(&s,&master,&ip);
 save(dir,"init-0.bin",&snapshots[0],340);save(dir,"init-1.bin",&snapshots[1],340);
 for(unsigned v=1;v<=2;v++){for(unsigned on=0;on<=1;on++){
  initialize(&s,&master,&ip);remote.version=v;remote.features=1;remote.flags=1;s.in=&remote;
  qcom_smp2p_negotiate(&s);if(!s.negotiation_done)qcom_smp2p_negotiate(&s);assert(s.negotiation_done);
  bool ack=qcom_smp2p_check_ssr(&s);struct ipa ipa={0};struct ipa_smp2p power={.ipa=&ipa,.valid_state=&ip,.enabled_state=&ip,.valid_bit=0,.enabled_bit=1};
  power_active=on;ipa_smp2p_notify(&power);assert(power.notified);ipa_smp2p_notify(&power);if(ack)qcom_smp2p_do_ssr_ack(&s);
  assert(local.entries[0].value==0);assert(local.entries[1].value==(on?3U:1U));assert(local.flags==2);
  char name[64];snprintf(name,sizeof(name),"v%u-on%u.bin",v,on);save(dir,name,&local,340);
  snprintf(name,sizeof(name),"v%u-on%u-kicks.bin",v,on);save(dir,name,snapshots,kicks*340);
 }}puts("PASS original Linux SMP2P initialization, downgrade/feature negotiation, IPA power replies and initial restart acknowledgement");}
'''
    (OUT/'oracle.c').write_text(c)
    for args,label in [(['cc','-std=gnu11','-O2','-Wall','-Wextra','-Werror',str(OUT/'oracle.c'),'-o',str(OUT/'oracle')],'build'),([str(OUT/'oracle'),str(OUT)],'run')]:
        r=subprocess.run(args,cwd=ROOT,capture_output=True,text=True);(OUT/(label+'.log')).write_text(r.stdout+r.stderr)
        if r.returncode:raise SystemExit(f'{label} failed: {OUT/(label+".log")}')
    result={'result':'passed','kernel_commit':PIN,'archive_sha256':ARCHIVE_HASH,
            'source_sha256':{p:hashlib.sha256(s.encode()).hexdigest()for p,s in sources.items()},
            'oracle_sha256':sha(OUT/'oracle.c'),'binary_fixtures_sha256':{p.name:sha(p)for p in OUT.glob('*.bin')},
            'scope':'Original Linux C AP wire bytes and notification sequence only; scheduling/IRQ service and physical modem startup are not proved.',
            'apcs_register_physical':0x17c0000c,'apcs_smp2p_bit':14,'physical_smp2p_irq':483,'device_actions':[]}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n');print('PASS: original pinned Linux SMP2P C wire and doorbell fixtures. No device action.')
if __name__=='__main__':main()
