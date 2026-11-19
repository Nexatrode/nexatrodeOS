#!/usr/bin/env python3
"""Compile the selected original Linux negotiation/control encoders."""
from pathlib import Path
import hashlib,importlib.util,json,os,re,subprocess,tarfile
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-glink-transport-linux'
PIN='2267e248204e7e051e0f7355298949a6440de579'
ARCHIVE_HASH='d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
spec=importlib.util.spec_from_file_location('layout',Path(__file__).with_name('audit-native-mpss-smem-layout.py'))
layout=importlib.util.module_from_spec(spec);spec.loader.exec_module(layout)
def main():
    os.umask(0o077);OUT.mkdir(mode=0o700,parents=True,exist_ok=True)
    if OUT.stat().st_mode&0o077:raise ValueError('private output required')
    archive=ROOT/'.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if sha(archive)!=ARCHIVE_HASH:raise ValueError('wrong pinned source archive')
    sources={}
    with tarfile.open(archive)as t:
        for name in ['drivers/rpmsg/qcom_glink_native.c','drivers/rpmsg/qcom_glink_native.h']:
            data=t.extractfile(f'linux-{PIN}/{name}').read();sources[name]=data.decode()
            p=OUT/'linux-source'/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
    glink,header=sources.values()
    assert re.search(r'#define\s+GLINK_FEATURE_INTENT_REUSE\s+BIT\(0\)',header)
    remote=layout.block(glink,'static int qcom_glink_create_remote(')
    local=layout.block(glink,'static struct glink_channel *qcom_glink_create_local(')
    assert remote.index('qcom_glink_send_open_ack(')<remote.index('qcom_glink_send_open_req(')
    assert local.index('qcom_glink_send_open_req(')<local.index('wait_for_completion_timeout(&channel->open_ack')<local.index('qcom_glink_send_open_ack(')
    c=r'''
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>
typedef uint8_t u8;typedef uint32_t u32;typedef uint16_t __le16;typedef uint32_t __le32;
#define __packed __attribute__((packed))
#define __struct_group(TAG,NAME,ATTRS,...) union{struct{__VA_ARGS__}ATTRS;struct TAG{__VA_ARGS__}ATTRS NAME;}ATTRS
#define cpu_to_le16(v) (v)
#define cpu_to_le32(v) (v)
#define fallthrough __attribute__((fallthrough))
#define trace_qcom_glink_cmd_version_tx(...) ((void)0)
#define trace_qcom_glink_cmd_version_ack_tx(...) ((void)0)
#define trace_qcom_glink_cmd_version_rx(...) ((void)0)
#define trace_qcom_glink_cmd_version_ack_rx(...) ((void)0)
#define trace_qcom_glink_cmd_open_ack_tx(...) ((void)0)
#define trace_qcom_glink_cmd_open_tx(...) ((void)0)
#define spin_lock_irqsave(l,f) ((void)(l),(f)=0)
#define spin_unlock_irqrestore(l,f) ((void)(l),(void)(f))
#define DEFINE_RAW_FLEX(t,n,m,N) unsigned char storage_##n[sizeof(t)+(N)] __attribute__((aligned(8)))={0};t*n=(void*)storage_##n
#define ALIGN(v,a) (((v)+(a)-1)&~((a)-1))
#define GFP_ATOMIC 0
struct qcom_glink{u32 features;int idr_lock,lcids;};
struct glink_channel{int refcount,lcid,rcid;char name[32];};
static void kref_get(int*p){++*p;}
static int idr_alloc_cyclic(int*p,void*c,int min,int max,int g){(void)p;(void)c;(void)g;assert(min==1&&max==65536);return 1;}
static void idr_remove(int*p,int id){(void)p;(void)id;}
static unsigned char last[64];static size_t count;
static int qcom_glink_tx(struct qcom_glink*g,const void*h,size_t hn,const void*d,size_t dn,bool wait){(void)g;assert(wait&&hn+dn<=sizeof(last));memcpy(last,h,hn);if(dn)memcpy(last+hn,d,dn);count=hn+dn;return 0;}
'''
    for name in ['GLINK_VERSION_1','GLINK_NAME_SIZE','RPM_GLINK_CID_MIN','RPM_GLINK_CID_MAX',
                 'GLINK_CMD_VERSION','GLINK_CMD_VERSION_ACK','GLINK_CMD_OPEN','GLINK_CMD_OPEN_ACK']:
        c+=re.search(r'^#define\s+'+name+r'\s+[^\n]+',glink,re.M)[0]+'\n'
    c+=layout.block(glink,'struct glink_msg {').rstrip()+' __packed;\n'
    for sig in ['static int qcom_glink_send_version(','static void qcom_glink_send_version_ack(',
                'static void qcom_glink_send_open_ack(','static int qcom_glink_send_open_req(',
                'static void qcom_glink_receive_version(','static void qcom_glink_receive_version_ack(']:c+=layout.block(glink,sig)
    c+=r'''
static void save(const char*dir,const char*name){char p[4096];assert(snprintf(p,sizeof(p),"%s/%s",dir,name)>0);FILE*f=fopen(p,"wb");assert(f&&count);assert(fwrite(last,1,count,f)==count);assert(fclose(f)==0);count=0;}
int main(int argc,char**argv){assert(argc==2);struct qcom_glink g={.features=1};
 assert(qcom_glink_send_version(&g)==0);save(argv[1],"version-1.bin");
 qcom_glink_receive_version(&g,1,7);save(argv[1],"ack-1.bin");
 qcom_glink_receive_version_ack(&g,1,1);assert(count==0&&g.features==1);
 qcom_glink_receive_version_ack(&g,1,0);save(argv[1],"counteroffer-0.bin");assert(g.features==0);
 qcom_glink_receive_version(&g,1,7);save(argv[1],"ack-0.bin");assert(g.features==0);
 struct glink_channel channel={.rcid=37};strcpy(channel.name,"IPCRTR");
 qcom_glink_send_open_ack(&g,&channel);save(argv[1],"open-ack-37.bin");
 assert(qcom_glink_send_open_req(&g,&channel)==0&&channel.lcid==1);save(argv[1],"open-ipcrtr-1.bin");
 puts("PASS original Linux version/intersection/counteroffer and channel wire functions");}
'''
    (OUT/'oracle.c').write_text(c)
    for args,label in [(['cc','-std=gnu11','-Wall','-Wextra','-Werror','-Wno-pointer-sign','-O2',str(OUT/'oracle.c'),'-o',str(OUT/'oracle')],'build'),([str(OUT/'oracle'),str(OUT)],'run')]:
        p=subprocess.run(args,cwd=ROOT,capture_output=True,text=True);(OUT/(label+'.log')).write_text(p.stdout+p.stderr)
        if p.returncode:raise SystemExit(f'{label} failed: {OUT/(label+".log")}')
    result={'result':'passed','kernel_commit':PIN,'archive_sha256':ARCHIVE_HASH,
        'source_sha256':{n:hashlib.sha256(s.encode()).hexdigest()for n,s in sources.items()},'oracle_sha256':sha(OUT/'oracle.c'),
        'fixtures_sha256':{p.name:sha(p)for p in OUT.glob('*.bin')},'device_actions':[],
        'scope':'Original Linux control encoders and feature handlers in host RAM; stub allocation gives the native single configured local channel ID. OPEN padding is zeroed by the test allocation, not a protocol requirement.'}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n');print('PASS: original pinned Linux GLINK negotiation/control fixtures. No device action.')
if __name__=='__main__':main()
