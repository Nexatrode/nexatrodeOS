#!/usr/bin/env python3
"""Offline full exception-RX initial replenish and default route, pinned Linux.

No device operations. Compile original channel START, RX replenish/page-add,
transaction/TRE commit, suspend-enable and default-route functions. Kernel
allocation, IRQ wait, DMA-map and transaction bookkeeping are host stand-ins.
This planner supplies no native mappings, pool owner or hardware readiness.
"""
from pathlib import Path
import hashlib,importlib.util,json,os,re,shutil,subprocess,tarfile
ROOT=Path(__file__).resolve().parents[2]
HERE=Path(__file__).resolve().parent
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-receive'
IPA='drivers/net/ipa/'
def module(name,file):
    s=importlib.util.spec_from_file_location(name,HERE/file);m=importlib.util.module_from_spec(s);s.loader.exec_module(m);return m
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,name,env=None,expected=0):
    p=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True)
    (OUT/(name+'.log')).write_text(p.stdout+p.stderr)
    if p.returncode!=expected:raise SystemExit(f'{name}: exit {p.returncode}; see {OUT/(name+".log")}')
    return p
def fn(mem,s,name):
    return mem.definition(s,r'^(?:static\s+)?(?:void|void \*|int|u32|bool|__le16|__le32|struct gsi_trans \*)\s*'+re.escape(name)+r'\(')+'\n'
def start_oracle():
    # The preceding command fixture is frozen, source-bound and already
    # compiles the original full START and channel-control ISR paths.
    cached=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-command'
    manifest=json.loads((cached/'manifest.json').read_text())
    assert sha(cached/'linux-command-oracle.c')==manifest['artifacts']['linux-command-oracle.c']['sha256']
    assert all(sha(ROOT/n)==h for n,h in manifest['source_sha256'].items())
    c=(cached/'linux-command-oracle.c').read_text()
    old='value != 0x01000001';assert c.count(old)==1
    c=c.replace(old,'(value != 0x01000001 && value != 0x01000002)')
    before,tail=c.rsplit('}',1)
    return before+'''    setup_result=4;
    if (gsi_channel_start(&g,2)) abort();
    if (registers[0x1f088/4]!=0x4c || registers[0x1f0b8/4]!=3 ||
        ((registers[0x1c100/4]>>20)&15)!=2) abort();
}'''+tail
def receive_oracle(source,mem,g,tables):
    # Reuse the original headers, register descriptors and v4.2 endpoint
    # declarations, stopping before that oracle's host memory model.
    c=tables.oracle(source,mem,g).split('struct gsi_ring {',1)[0]
    unused=mem.definition(source[IPA+'data/ipa_data-v4.2.c'],r'^static const struct ipa_mem ipa_mem_local_data\[\]\s*=\s*\{')+';'
    assert c.count(unused)==1;c=c.replace(unused,'')
    c+=r'''
#define NET_SKB_PAD 64 /* exact reference-ABI measurement, checked below */
#define atomic_read(p) (*(p))
#define test_bit(n,p) (!!((p)[0]&(1UL<<(n))))
#define set_bit(n,p) ((p)[0]|=1UL<<(n))
#define clear_bit(n,p) ((p)[0]&=~(1UL<<(n)))
#define __change_bit(n,p) ((p)[0]^=1UL<<(n))
#define bitmap_weight(p,n) ((unsigned long)__builtin_popcountl((p)[0]&((1UL<<(n))-1)))
static bool test_and_set_bit(unsigned n,unsigned long *p){bool old=test_bit(n,p);set_bit(n,p);return old;}
enum dma_data_direction {DMA_TO_DEVICE,DMA_FROM_DEVICE};
struct gsi_ring {u64 addr;void *virt;u32 count,index;};
struct gsi;
struct gsi_trans_info {int tre_avail;};
struct gsi_channel {struct gsi *gsi;u32 id,tre_count,trans_tre_max;bool toward_ipa,command;struct gsi_ring tre_ring;struct gsi_trans_info trans_info;};
struct gsi {void *dev,*virt;struct gsi_channel channel[3];};
struct ipa_interrupt;
struct ipa {enum ipa_version version;void *reg_virt;struct gsi gsi;struct ipa_interrupt *interrupt;unsigned long available[1];u32 endpoint_count;};
struct ipa_interrupt {struct ipa *ipa;unsigned long suspend_enabled[1];u32 enabled;};
enum ipa_replenish_flag {IPA_REPLENISH_ENABLED,IPA_REPLENISH_ACTIVE};
struct ipa_endpoint {struct ipa *ipa;u32 endpoint_id,channel_id;bool toward_ipa;struct ipa_endpoint_config config;unsigned long replenish_flags[1];u64 replenish_count;int replenish_work;};
struct page {u64 iova;};
struct scatterlist {struct page *page;u32 size,offset;u64 dma;};
struct gsi_trans {struct gsi *gsi;u32 channel_id,rsvd_count,used_count,len;enum dma_data_direction direction;struct scatterlist sgl[1];u8 cmd_opcode[1];void *data;};
static u8 mmio[0x20000];static u32 values[0x20000/4];
static u32 allocations,transactions,commits,doorbells,mapped;
static struct page pages[256];static struct gsi_trans trans_pool[256];static struct gsi_tre tres[256];
const struct reg *ipa_reg(struct ipa *i,enum ipa_reg_id id){(void)i;return reg(&ipa_regs_v4_2,id);}
const struct reg *gsi_reg(struct gsi *g,enum gsi_reg_id id){(void)g;return reg(&gsi_regs_v4_0,id);}
static u32 ioread32(void *a){size_t o=(u8*)a-mmio;if(o>=sizeof(mmio)||o%4)abort();return values[o/4];}
static void iowrite32(u32 v,void *a){size_t o=(u8*)a-mmio;if(o>=sizeof(mmio)||o%4)abort();values[o/4]=v;printf("write %05zx %08x\n",o,v);if(o==0x1e010)doorbells++;}
static u32 gsi_channel_id(struct gsi_channel *c){return c->id;}
static unsigned get_order(u32 bytes){unsigned o=0;while((4096U<<o)<bytes)o++;return o;}
static struct page *dev_alloc_pages(unsigned order){if(order!=1||allocations>=251)abort();struct page *p=&pages[allocations];p->iova=UINT64_C(0x80200000)+8192*allocations++;return p;}
static void put_page(struct page *p){(void)p;abort();}
static void sg_set_page(struct scatterlist *sg,struct page *p,u32 size,u32 offset){sg->page=p;sg->size=size;sg->offset=offset;}
static int dma_map_sg(void *d,struct scatterlist *sg,u32 count,enum dma_data_direction dir){(void)d;if(count!=1||dir!=DMA_FROM_DEVICE||sg->offset!=NET_SKB_PAD)abort();sg->dma=sg->page->iova+sg->offset;mapped++;return 1;}
#define sg_dma_address(s) ((s)->dma)
#define sg_dma_len(s) ((s)->size)
#define for_each_sg(first,sg,n,i) for((i)=0,(sg)=(first);(i)<(n);(i)++,(sg)++)
static void gsi_trans_map(struct gsi_trans *t,u32 index){if(t->used_count!=1||index!=commits)abort();}
static void gsi_trans_move_committed(struct gsi_trans *t){(void)t;commits++;}
static void gsi_trans_move_pending(struct gsi_trans *t){(void)t;}
static void gsi_trans_tx_committed(struct gsi_trans *t){(void)t;abort();}
static void gsi_trans_tx_queued(struct gsi_trans *t){(void)t;abort();}
static void gsi_trans_free(struct gsi_trans *t){(void)t;abort();}
static bool gsi_channel_trans_idle(struct gsi *g,u32 id){return !transactions&&g->channel[id].trans_info.tre_avail>0;}
static void schedule_delayed_work(int *w,u32 delay){(void)w;(void)delay;abort();}
#define msecs_to_jiffies(n) (n)
static struct gsi_trans *gsi_channel_trans_alloc(struct gsi *g,u32 id,u32 count,enum dma_data_direction dir){
 if(id!=2||count!=1||dir!=DMA_FROM_DEVICE)abort();
 if(!g->channel[id].trans_info.tre_avail)return NULL;
 g->channel[id].trans_info.tre_avail--;
 struct gsi_trans *t=&trans_pool[transactions++];t->gsi=g;t->channel_id=id;t->rsvd_count=count;t->direction=dir;return t;
}
'''
    trans=source[IPA+'gsi_trans.c'];endpoint=source[IPA+'ipa_endpoint.c'];intr=source[IPA+'ipa_interrupt.c']
    for name in ['gsi_ring_virt','gsi_ring_addr','gsi_channel_doorbell','gsi_channel_tre_max']:
        c+=fn(mem,source[IPA+'gsi.c'],name)
    for name in ['gsi_trans_page_add','gsi_tre_len_opcode','gsi_tre_flags','gsi_trans_tre_fill','__gsi_trans_commit','gsi_trans_commit']:
        c+=fn(mem,trans,name)
    c+=fn(mem,endpoint,'ipa_endpoint_trans_alloc')
    c+=re.search(r'^#define IPA_REPLENISH_BATCH[^\n]*',endpoint,re.M)[0]+'\n'
    for name in ['ipa_endpoint_replenish_one','ipa_endpoint_replenish','ipa_endpoint_replenish_enable','ipa_endpoint_default_route_set']:
        c+=fn(mem,endpoint,name)
    for name in ['ipa_interrupt_enabled_update','ipa_interrupt_enable','ipa_interrupt_disable','ipa_interrupt_suspend_control','ipa_interrupt_suspend_enable']:
        c+=fn(mem,intr,name)
    c+=r'''
int main(void){
 struct ipa i={.version=IPA_VERSION_4_2,.reg_virt=mmio,.endpoint_count=17,.available={0xf72}};
 struct ipa_interrupt intr={.ipa=&i,.enabled=BIT(IPA_IRQ_UC_0)|BIT(IPA_IRQ_UC_1)};i.interrupt=&intr;
 i.gsi.virt=mmio;
 const struct ipa_gsi_endpoint_data *d=&ipa_gsi_endpoint_data[IPA_ENDPOINT_AP_LAN_RX];
 struct gsi_channel *ch=&i.gsi.channel[d->channel_id];
 ch->gsi=&i.gsi;ch->id=d->channel_id;ch->tre_count=d->channel.tre_count;ch->trans_tre_max=d->channel.tlv_count;
 ch->tre_ring=(struct gsi_ring){.addr=UINT64_C(0x80009000),.virt=tres,.count=ch->tre_count};
 ch->trans_info.tre_avail=gsi_channel_tre_max(&i.gsi,ch->id);
 struct ipa_endpoint e={.ipa=&i,.endpoint_id=d->endpoint_id,.channel_id=d->channel_id,.toward_ipa=d->toward_ipa,.config=d->endpoint.config};
 if(!is_power_of_2(e.config.rx.buffer_size) ||
    reg_field_max(ipa_reg(&i,ROUTE),ROUTE_DEF_PIPE)!=31 ||
    reg_decode(ipa_reg(&i,ROUTE),ROUTE_DEF_PIPE,0x01100050)!=8){dev_err(NULL,"bad RX oracle geometry");abort();}
 printf("geometry %u %u %u %u %u %u\n",d->endpoint_id,d->channel_id,d->channel.tre_count,d->channel.tlv_count,ch->trans_info.tre_avail,e.config.rx.buffer_size);
 ipa_interrupt_suspend_enable(&intr,e.endpoint_id);
 ipa_endpoint_replenish_enable(&e);
 ipa_endpoint_default_route_set(&i,e.endpoint_id);
 if(allocations!=251||transactions!=251||commits!=251||mapped!=251||doorbells!=16||ch->trans_info.tre_avail)abort();
 for(u32 j=0;j<transactions;j++){
  struct gsi_trans *t=&trans_pool[j];printf("buffer %u %016llx %u %u ",j,(unsigned long long)pages[j].iova,t->sgl[0].offset,t->sgl[0].size);
  const u8 *b=(const u8*)&tres[j];for(u32 k=0;k<16;k++)printf("%02x",b[k]);puts("");
 }
}
'''
    return c
def main():
    OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    g=module('receive_gsi','test-native-ipa-linux-gsi.py');mem=module('receive_mem','test-native-ipa-linux-memsetup.py')
    tables=module('receive_tables','test-native-ipa-linux-tables.py')
    assert sha(g.ARCHIVE)==g.ARCHIVE_HASH
    inputs=list(dict.fromkeys([*mem.INPUTS,IPA+'ipa_version.h',IPA+'ipa_endpoint.h',IPA+'ipa_endpoint.c',IPA+'ipa_data.h',IPA+'ipa_table.c',IPA+'ipa_interrupt.c']))
    source={}
    with tarfile.open(g.ARCHIVE)as tf:
        prefix=tf.getnames()[0].split('/')[0]+'/'
        for rel in inputs:
            b=tf.extractfile(prefix+rel).read();p=OUT/'linux-source'/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b);source[rel]=b.decode()
    enable=fn(mem,source[IPA+'ipa_endpoint.c'],'ipa_endpoint_enable_one')
    order=['gsi_channel_start(gsi, endpoint->channel_id)','ipa_interrupt_suspend_enable(ipa->interrupt, endpoint_id)',
        'ipa_endpoint_replenish_enable(endpoint)']
    assert [enable.index(n)for n in order]==sorted(set(enable.index(n)for n in order))
    abi=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-endpoint/reference-abi.json'
    abi_values=json.loads(abi.read_text())
    assert abi_values['page_bytes']==4096 and abi_values['net_skb_pad']==64 and abi_values['skb_shared_info_aligned']==320
    cc=shutil.which('clang')or shutil.which('cc');assert cc
    for name,c in [('start',start_oracle()),('replenish',receive_oracle(source,mem,g,tables))]:
        p=OUT/(name+'-oracle.c');p.write_text(c)
        run([cc,'-std=gnu11','-Wall','-Wextra','-Werror',str(p),'-o',str(OUT/(name+'-oracle'))],name+'-c-build')
        rows=run([str(OUT/(name+'-oracle'))],name+'-c-oracle').stdout
        if name=='start':rows='\n'.join(' '.join(r.split()[2:])for r in rows.splitlines()if r.startswith('setup 4 '))+'\n'
        (OUT/('linux-receive-'+name+'.txt')).write_text(rows)
    env=dict(os.environ,SUNFISH_RECEIVE_START_C=str(OUT/'linux-receive-start.txt'),SUNFISH_RECEIVE_C=str(OUT/'linux-receive-replenish.txt'))
    rustc=shutil.which('rustc');assert rustc
    binary=OUT/'receive-test';test=ROOT/'tests/sunfish_ipa_linux_receive_test.rs'
    run([rustc,'--edition=2024','-D','warnings','-C','opt-level=2','--test',str(test),'-o',str(binary)],'rust-build',env)
    tested=run([str(binary),'receive_'],'rust-tests',env)
    groups=int(re.search(r'test result: ok\. (\d+) passed;',tested.stdout)[1])
    names=['sunfish_ipa_linux_buffers','sunfish_ipa_linux_gsi','sunfish_ipa_linux_endpoint','sunfish_ipa_linux_receive','sunfish_ipa_linux_receive_pool']
    wrapper=OUT/'core.rs';wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};'for n in names))
    run([rustc,'--edition=2024','-D','warnings','--crate-type=lib','--target=aarch64-unknown-none',str(wrapper),'-o',str(OUT/'receive-aarch64.rlib')],'aarch64-core')
    original=(ROOT/'rust/src/sunfish_ipa_linux_receive.rs').read_text();harness=test.read_text()
    mutations={'wrong-headroom':('pub const HEADROOM: usize = 64;','pub const HEADROOM: usize = 32;'),
        'wrong-batch':('pub const BATCH: usize = 16;','pub const BATCH: usize = 8;'),
        'wrong-rx-bei':('0x00020200u32.to_le_bytes()','0x00020600u32.to_le_bytes()'),
        'wrong-start-channel':('g(0x1f008, 0x01000002)','g(0x1f008, 0x01000001)')}
    for label,(before,after)in mutations.items():
        assert original.count(before)==1,before
        mutant=OUT/(label+'.rs');mutant.write_text(original.replace(before,after))
        body=harness
        for n in names:
            p=mutant if n=='sunfish_ipa_linux_receive'else ROOT/'rust/src'/(n+'.rs')
            body=body.replace('../rust/src/'+n+'.rs',str(p))
        p=OUT/(label+'-test.rs');p.write_text(body);exe=OUT/(label+'-test')
        run([rustc,'--edition=2024','-D','warnings','--test','--crate-name','receive_mutation',str(p),'-o',str(exe)],label+'-build',env)
        run([str(exe),'--exact','receive_full_linux_start_suspend_replenish_and_default_route_match_compiled_c'],label,env,expected=101)
    selected=['rust/src/'+n+'.rs'for n in names]+['tests/sunfish_ipa_linux_receive_test.rs',
        'platforms/sunfish/test-native-ipa-linux-receive.py','platforms/sunfish/test-native-ipa-linux-gsi.py',
        'platforms/sunfish/test-native-ipa-linux-command.py','platforms/sunfish/test-native-ipa-linux-memsetup.py','platforms/sunfish/test-native-ipa-linux-tables.py',
        'docs/sunfish-ipa-receive-plan.md']
    for rel in selected:
        p=OUT/'source'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/rel,p)
    manifest={'kernel_commit':g.PIN,'archive_sha256':g.ARCHIVE_HASH,
        'reference_sha256':{rel:sha(OUT/'linux-source'/rel)for rel in source},
        'source_sha256':{rel:sha(ROOT/rel)for rel in selected},'reference_kernel_abi_sha256':sha(abi),
        'command_oracle_template_sha256':sha(ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-command/linux-command-oracle.c'),
        'behavioral_groups':groups,'planner_groups':4,'pool_groups':5,'encoder_mutations_rejected':len(mutations),'aarch64_no_std':True,
        'receive_buffers':251,'buffer_bytes':8192,'dma_offset':64,'dma_bytes':8128,'receive_backing_pages':502,
        'initial_replenish_doorbells':16,'composed_c_start_writes':8,'composed_c_suspend_route_writes':3,
        'library_exported':'pub mod sunfish_ipa_linux_receive;' in (ROOT/'rust/src/lib.rs').read_text(),'hardware_commands':[],
        'limits':'Offline initial receive planner only. Needs real retained RX backing, SMMU installation, pool/controller and interrupt/completion/lifecycle integration; no native RX or readiness proof.'}
    manifest['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)}for p in OUT.iterdir()if p.is_file()and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'PASS: {groups} receive groups, 251 exact compiled-C TREs, 16 replenish doorbells, 8 START + 3 suspend/route writes, {len(mutations)} rejected mutations, AArch64 no_std; {OUT}')
if __name__=='__main__':main()
