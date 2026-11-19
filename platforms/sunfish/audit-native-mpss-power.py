#!/usr/bin/env python3
"""Original pinned rpmhpd clamp and q6v5/qmp ON payload; CPU only."""
from pathlib import Path
import hashlib,importlib.util,json,os,subprocess,tarfile
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-power-linux'
PIN='2267e248204e7e051e0f7355298949a6440de579';ARCHIVE_HASH='d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
spec=importlib.util.spec_from_file_location('layout',Path(__file__).with_name('audit-native-mpss-smem-layout.py'));layout=importlib.util.module_from_spec(spec);spec.loader.exec_module(layout)
def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    archive=ROOT/'.cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz'
    if OUT.stat().st_mode&0o077 or sha(archive)!=ARCHIVE_HASH:raise ValueError('private output and exact Linux required')
    names=['drivers/pmdomain/qcom/rpmhpd.c','drivers/clk/qcom/clk-rpmh.c','drivers/remoteproc/qcom_q6v5.c','drivers/remoteproc/qcom_q6v5_pas.c','drivers/soc/qcom/qcom_aoss.c','arch/arm64/boot/dts/qcom/sm7150.dtsi']
    s={}
    with tarfile.open(archive)as t:
        for name in names:
            b=t.extractfile(f'linux-{PIN}/{name}').read();s[name]=b.decode();p=OUT/'linux-source'/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b)
    pd,clk,q6,pas,qmp,dt=(s[n]for n in names)
    mpss=layout.block(dt,'remoteproc_mpss: remoteproc@4080000 {')
    for token in ['clocks = <&rpmhcc RPMH_CXO_CLK>;','clock-names = "xo";','qcom,qmp = <&aoss_qmp>;','power-domain-names = "cx",','"mx",','"mss";']:assert token in mpss,token
    for absent in ['interconnects =','aggre2','cx-supply','px-supply','iommus =']:assert absent not in mpss,absent
    assert '.parent = &mx.pd' in layout.block(pd,'static struct rpmhpd cx_w_mx_parent =')
    assert '[RPMHPD_CX] = &cx_w_mx_parent' in layout.block(pd,'static struct rpmhpd *sm7150_rpmhpds[] =')
    assert '.pas_id = 4' in layout.block(pas,'static const struct qcom_pas_data mpss_resource_init =')
    assert '"cx",\n\t\t"mss",' in layout.block(pas,'static const struct qcom_pas_data mpss_resource_init =')
    assert 'DEFINE_CLK_RPMH_ARC(bi_tcxo, "xo.lvl", 0x3,' in clk
    start=layout.block(pas,'static int qcom_pas_start(')
    positions=[start.index(n)for n in ['qcom_q6v5_prepare(', 'qcom_pas_pds_enable(', 'clk_prepare_enable(pas->xo)', 'qcom_mdt_pas_load(', 'qcom_scm_pas_prepare_and_auth_reset(pas->pas_ctx)']]
    assert positions==sorted(positions)
    c=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <assert.h>
#include <errno.h>
#include <stddef.h>
#include <limits.h>
typedef uint16_t u16;typedef uint32_t u32;
#define RPMH_ARC_MAX_LEVELS 32
#define RPMH_REGULATOR_LEVEL_RETENTION 16
#define RPMH_ACTIVE_ONLY_STATE 2
#define RPMH_WAKE_ONLY_STATE 1
#define RPMH_SLEEP_STATE 0
#define BIT(n) (1U<<(n))
#define max(a,b) ((a)>(b)?(a):(b))
#define IS_ERR(p) ((intptr_t)(p)<0)
#define PTR_ERR(p) ((intptr_t)(p))
#define IS_ERR_OR_NULL(p) (!(p)||IS_ERR(p))
#define container_of(p,t,m) ((t*)((char*)(p)-offsetof(t,m)))
#define domain_to_rpmhpd(p) container_of(p,struct rpmhpd,pd)
#define guard(t) (void)
#define mutex_lock(l) ((void)(l))
#define mutex_unlock(l) ((void)(l))
#define dev_err(d,...) ((void)(d))
#define pr_debug(...) ((void)0)
#define __printf(a,b)
#define WARN_ON(x) (x)
#define trace_aoss_send(b) ((void)(b))
#define trace_aoss_send_done(b,r) ((void)(b),(void)(r))
#define QMP_MSG_LEN 64
#define HZ 100
struct generic_pm_domain{const char*name;};
struct device{};
struct tcs_cmd{u32 addr,data;bool wait;};
static int rpmhpd_lock;static u16 aux[32];static size_t auxn;static u32 last_corner;
static const u16*cmd_db_read_aux_data(const char*n,size_t*len){assert(!strcmp(n,"cx.lvl"));*len=auxn*2;return aux;}
static int rpmh_write(struct device*d,int state,struct tcs_cmd*c,unsigned n){(void)d;assert(state==2&&n==1&&!c->wait);last_corner=c->data;return 0;}
static int rpmh_write_async(struct device*d,int state,struct tcs_cmd*c,unsigned n){(void)d;(void)state;(void)c;(void)n;return 0;}
'''
    c+=layout.block(pd,'struct rpmhpd {')
    for sig in ['static int rpmhpd_send_corner(','static void to_active_sleep(','static int rpmhpd_aggregate_corner(','static int rpmhpd_power_on(','static int rpmhpd_set_performance_state(','static int rpmhpd_update_level_mapping(']:c+=layout.block(pd,sig)
    c+=r'''
struct qmp{void*dev;unsigned char*msgram;unsigned offset;int tx_lock,event;};
struct qcom_q6v5{struct qmp*qmp;const char*load_state;void*dev;};
static unsigned char packet[68];
static void __iowrite32_copy(void*d,const void*s,size_t n){memcpy(d,s,n*4);}
static void writel(u32 v,void*p){memcpy(p,&v,4);}
static u32 readl(const void*p){u32 v;memcpy(&v,p,4);return v;}
static void qmp_kick(struct qmp*q){assert(readl(q->msgram+q->offset)==64);memcpy(packet,q->msgram+q->offset,68);writel(0,q->msgram+q->offset);}
#define wait_event_interruptible_timeout(e,condition,t) ((void)(e),(void)(t),assert(condition),1L)
'''
    c+=layout.block(qmp,'static bool qmp_message_empty(')
    c+=layout.block(qmp,'int __printf(2, 3) qmp_send(')
    c+=layout.block(q6,'static int q6v5_load_state_toggle(')
    c+=r'''
int main(int argc,char**argv){assert(argc==2);char path[4096];snprintf(path,sizeof(path),"%s/levels.txt",argv[1]);FILE*f=fopen(path,"w");assert(f);
 for(unsigned n=2;n<=32;n++){for(unsigned kind=0;kind<3;kind++){
  memset(aux,0,sizeof(aux));for(unsigned j=1;j<n;j++)aux[j]=kind==0?j*48:kind==1?48:512-j;
  auxn=32;struct rpmhpd pd={.res_name="cx.lvl",.state_synced=true,.addr=0x30000};
  assert(rpmhpd_update_level_mapping(&pd)==0);assert(pd.level_count==n);assert(rpmhpd_set_performance_state(&pd.pd,INT_MAX)==0);assert(rpmhpd_power_on(&pd.pd)==0);assert(last_corner==n-1);
  fprintf(f,"%zu",auxn);for(unsigned j=0;j<auxn;j++)fprintf(f," %u",aux[j]);fprintf(f," %u %u %u\n",last_corner,pd.level[last_corner],pd.enable_corner);
 }}assert(fclose(f)==0);
 unsigned char ram[128]={0};struct qmp q={.msgram=ram,.offset=4};struct qcom_q6v5 q6={.qmp=&q,.load_state="modem"};assert(q6v5_load_state_toggle(&q6,true)==0);
 snprintf(path,sizeof(path),"%s/on-packet.bin",argv[1]);f=fopen(path,"wb");assert(f);assert(fwrite(packet,1,sizeof(packet),f)==sizeof(packet));assert(fclose(f)==0);
 puts("PASS original rpmhpd level/clamp/power-on functions (93 maps) and q6v5/qmp ON packet");}
'''
    (OUT/'oracle.c').write_text(c)
    for args,label in [(['cc','-std=gnu11','-O2','-Wall','-Wextra','-Werror','-Wno-sign-compare',str(OUT/'oracle.c'),'-o',str(OUT/'oracle')],'build'),([str(OUT/'oracle'),str(OUT)],'run')]:
        r=subprocess.run(args,cwd=ROOT,capture_output=True,text=True);(OUT/(label+'.log')).write_text(r.stdout+r.stderr)
        if r.returncode:raise SystemExit(f'{label} failed: {OUT/(label+".log")}')
    result={'result':'passed','kernel_commit':PIN,'archive_sha256':ARCHIVE_HASH,'source_sha256':{n:hashlib.sha256(s[n].encode()).hexdigest()for n in names},
            'oracle_sha256':sha(OUT/'oracle.c'),'levels_sha256':sha(OUT/'levels.txt'),'on_packet_sha256':sha(OUT/'on-packet.bin'),
            'level_cases':93,'qmp_payload_bytes':64,'proxy_domains':['cx','mss'],'cx_parent':'mx','xo_on':3,
            'absent_optional_mpss_properties':['interconnects','aggre2','cx-supply','px-supply','iommus'],
            'intentional_native_differences':['explicit maximum active-only MX parent floor','hold floors and QMP endpoint until reboot; no sleep/wake/off or handover unvote',
                'reject maps without a zero/off index or any nonzero corner','bounded work and terminal timeout; no Linux TX clearing/retry'],
            'scope':'CPU/source plan; not actual RSC ownership, rails, AOP semantic acceptance, PAS4 start or Wi-Fi proof.','device_actions':[]}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n');print('PASS: original Linux rpmhpd 93 maps, 64-byte QMP ON and selected Sunfish dependencies. No device action.')
if __name__=='__main__':main()
