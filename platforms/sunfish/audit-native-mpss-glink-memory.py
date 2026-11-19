#!/usr/bin/env python3
"""Compile the pinned original Linux SMEM FIFO functions in CPU RAM only."""
from pathlib import Path
import hashlib, importlib.util, json, os, subprocess, tarfile
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-glink-memory-linux'
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
    source={}
    with tarfile.open(archive)as t:
        for name in ['drivers/rpmsg/qcom_glink_smem.c','arch/arm64/boot/dts/qcom/sm7150.dtsi']:
            data=t.extractfile(f'linux-{PIN}/{name}').read();source[name]=data.decode()
            p=OUT/'linux-source'/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
    glink,dt=source.values()
    modem=layout.block(dt,'remoteproc_mpss: remoteproc@4080000 {')
    edge=layout.block(modem,'glink-edge {')
    assert 'GIC_SPI 449 IRQ_TYPE_EDGE_RISING' in edge and 'mboxes = <&apss_shared 12>;' in edge
    c=r'''
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef uint32_t u32;typedef uint32_t __le32;
#define container_of(p,t,m) ((t*)((char*)(p)-offsetof(t,m)))
#define IS_ERR(p) ((p)==NULL)
#define PTR_ERR(p) (-(long)ENOENT)
#define pr_err(...) ((void)0)
#define le32_to_cpu(v) (v)
#define cpu_to_le32(v) (v)
#define min_t(t,a,b) ((t)(a)<(t)(b)?(t)(a):(t)(b))
#define memcpy_fromio memcpy
#define ALIGN(v,a) (((v)+(a)-1)&~((a)-1))
#define wmb() __sync_synchronize()
struct qcom_glink_pipe {size_t length;};
struct qcom_glink_smem {unsigned remote_pid;};
struct glink_smem_pipe{struct qcom_glink_pipe native;__le32 *tail,*head;void *fifo;struct qcom_glink_smem *smem;};
static unsigned char tx[16384],rx[32768];static size_t rx_size;static bool published;
static void*qcom_smem_get(unsigned pid,unsigned id,size_t*len){assert(pid==1&&id==480);if(!published)return NULL;*len=rx_size;return rx;}
'''
    for line in glink.splitlines():
        if line.startswith('#define '):c+=line+'\n'
    for sig in ['static size_t glink_smem_rx_avail(', 'static void glink_smem_rx_peek(',
                'static void glink_smem_rx_advance(', 'static size_t glink_smem_tx_avail(',
                'static unsigned int glink_smem_tx_write_one(', 'static void glink_smem_tx_write(']:
        c+=layout.block(glink,sig)
    # Original descriptor assignments, copied verbatim from registration.
    a=glink.index('tx_pipe->tail = &descs[0];');b=glink.index('\n\tret = qcom_smem_alloc',a)
    assignment=glink[a:b]
    c+='static void bind(struct glink_smem_pipe*tx_pipe,struct glink_smem_pipe*rx_pipe,__le32*descs){\n'+assignment+'\n}\n'
    c+=r'''
int main(int argc,char**argv){assert(argc==2);
 for(unsigned size=4096;size<=32768;size*=2){if(size==8192)continue;
  memset(tx,0,sizeof(tx));memset(rx,0x5a,sizeof(rx));rx_size=size;published=false;
  __le32 desc[8]={0};struct qcom_glink_smem smem={.remote_pid=1};
  struct glink_smem_pipe t={.native.length=sizeof(tx),.fifo=tx,.smem=&smem},r={.smem=&smem};
  bind(&t,&r,desc);assert(glink_smem_rx_avail(&r.native)==0);
  desc[0]=128;desc[1]=16376;desc[2]=size-8;desc[3]=8;
  memcpy(rx+size-8,"ABCDEFGH",8);memcpy(rx,"IJKLMNOP",8);published=true;
  assert(glink_smem_rx_avail(&r.native)==16&&r.native.length==size);
  assert(glink_smem_tx_avail(&t.native)==120);
  glink_smem_tx_write(&t.native,"12345678",8,"abcdefgh",8);
  unsigned char readback[16];glink_smem_rx_peek(&r.native,readback,0,16);glink_smem_rx_advance(&r.native,16);
  assert(desc[0]==128&&desc[1]==8&&desc[2]==8&&desc[3]==8);
  char file[4096];assert(snprintf(file,sizeof(file),"%s/rx-%u.bin",argv[1],size)>0);
  FILE*f=fopen(file,"wb");assert(f);
  assert(fwrite(desc,1,32,f)==32&&fwrite(tx,1,sizeof(tx),f)==sizeof(tx));
  assert(fwrite(rx,1,size,f)==size&&fwrite(readback,1,16,f)==16);assert(fclose(f)==0);
 }puts("PASS original Linux descriptor ownership, lazy RX length and FIFO wrap effects");}
'''
    (OUT/'oracle.c').write_text(c)
    for args,label in [(['cc','-std=gnu11','-O2','-Wall','-Wextra','-Werror',str(OUT/'oracle.c'),'-o',str(OUT/'oracle')],'build'),([str(OUT/'oracle'),str(OUT)],'run')]:
        p=subprocess.run(args,cwd=ROOT,capture_output=True,text=True);(OUT/(label+'.log')).write_text(p.stdout+p.stderr)
        if p.returncode:raise SystemExit(f'{label} failed: {OUT/(label+".log")}')
    result={'result':'passed','kernel_commit':PIN,'archive_sha256':ARCHIVE_HASH,
            'source_sha256':{p:hashlib.sha256(s.encode()).hexdigest()for p,s in source.items()},
            'oracle_sha256':sha(OUT/'oracle.c'),'fixtures_sha256':{p.name:sha(p)for p in OUT.glob('rx-*.bin')},
            'physical_glink_irq':481,'apcs_glink_bit':12,'device_actions':[],
            'scope':'Original Linux descriptor/FIFO RAM effects only; no channel negotiation, IRQ delivery or physical MPSS proof.'}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print('PASS: original pinned Linux GLINK SMEM memory fixtures. No device action.')
if __name__=='__main__':main()
