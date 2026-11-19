#!/usr/bin/env python3
"""Pinned Linux table allocation/setup and six commands; offline only.

Compile the original v4.2 endpoint/memory data, table init/setup/validation,
command encoders, TRE encoders and ring doorbells using host allocator/MMIO
stand-ins. This does not issue device commands or establish DMA authority.
"""
from pathlib import Path
import hashlib,importlib.util,json,os,re,shutil,subprocess,tarfile

ROOT=Path(__file__).resolve().parents[2]
HERE=Path(__file__).resolve().parent
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-tables'
IPA='drivers/net/ipa/'
def module(name,file):
    s=importlib.util.spec_from_file_location(name,HERE/file);m=importlib.util.module_from_spec(s);s.loader.exec_module(m);return m
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,name,env=None,expected=0):
    p=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True);(OUT/(name+'.log')).write_text(p.stdout+p.stderr)
    if p.returncode!=expected:raise SystemExit(f'{name}: exit {p.returncode}; see {OUT/(name+".log")}')
    return p
def function(mem,s,name):
    return mem.definition(s,r'^(?:static\s+)?(?:void|int|u32|u64|bool|dma_addr_t|const struct ipa_mem \*)\s*'+re.escape(name)+r'\(')+'\n'
def oracle(source,mem,g):
    c=mem.oracle(source,g)
    extras=r'''
#define GENMASK_ULL(h,l) ((UINT64_MAX >> (63-(h))) & (UINT64_MAX << (l)))
#define u64_encode_bits(v,m) (((u64)(v) << __builtin_ctzll(m)) & (m))
#define max_t(t,x,y) ((t)(x)>(t)(y)?(t)(x):(t)(y))
#define hweight64(v) ((u32)__builtin_popcountll(v))
#define field_max(m) ((m) >> __builtin_ctzll(m))
#define BUILD_BUG_ON(c) _Static_assert(!(c), "build assertion")
#define GFP_KERNEL 0
#define ilog2(v) (63-__builtin_clzll(v))
'''
    c=c.replace('#define GSI_EE_AP 0',g.strip_includes(source[IPA+'ipa_version.h'])+extras)
    c=c.replace('#define dev_err(dev,...) ((void)(dev))',
        'static void table_log(void *dev,const char *fmt,...) { (void)dev;(void)fmt; }\n#define dev_err(dev,...) table_log(dev,__VA_ARGS__)')
    endpoint=source[IPA+'ipa_endpoint.h'];data=source[IPA+'ipa_data.h'];config=source[IPA+'data/ipa_data-v4.2.c']
    declarations=''
    for s,pattern in [(endpoint,r'^enum ipa_endpoint_name\s*\{'),
        *[(endpoint,r'^struct '+n+r'\s*\{') for n in ['ipa_endpoint_tx','ipa_endpoint_rx','ipa_endpoint_config']],
        *[(data,r'^struct '+n+r'\s*\{') for n in ['gsi_channel_data','ipa_endpoint_data','ipa_gsi_endpoint_data']],
        (config,r'^enum ipa_rsrc_group_id\s*\{'),
        (config,r'^static const struct ipa_gsi_endpoint_data ipa_gsi_endpoint_data\[\]\s*=\s*\{')]:
        declarations+=mem.definition(s,pattern)+';\n'
    before='struct gsi_ring { u64 addr; u32 count, index; };'
    assert c.count(before)==1;c=c.replace(before,declarations+'\n'+before)
    before='union ipa_cmd_payload {'
    table_payload=mem.definition(source[IPA+'ipa_cmd.c'],r'^struct ipa_cmd_hw_ip_fltrt_init\s*\{')+';\n'
    masks='\n'.join(l for l in source[IPA+'ipa_cmd.c'].splitlines() if l.startswith('#define IP_FLTRT_FLAGS_'))+'\n'
    assert c.count(before)==1;c=c.replace(before,table_payload+masks+before+' struct ipa_cmd_hw_ip_fltrt_init table_init;')
    replacements={
        'u64 zero_addr; };':'u64 zero_addr,filtered,table_addr; __le64 *table_virt; u32 route_count,filter_count,modem_route_count; enum ipa_version version; };',
        'struct entry entries[4];':'struct entry entries[8];',
        'payloads[4]':'payloads[8]',
        'if(allocations>=4)':'if(allocations>=8)',
        '0x80006000)+32*allocations':'0x80006000)+32*(4+allocations)',
        'if(count!=4 || transaction.count)':'if(count!=8 || transaction.count)',
        'if(t->count>=4 || size!=16)':'if(t->count>=8 || (size!=16 && size!=24))',
        'if(t->count!=4 || commits++)':'if(t->count!=6 || commits++)',
        '.count=256,.index=4':'.count=256,.index=10',
        'gsi_evt_ring_doorbell(t->gsi,0,1)':'gsi_evt_ring_doorbell(t->gsi,0,2)',
    }
    for before,after in replacements.items():assert c.count(before)==1,before;c=c.replace(before,after)
    table=source[IPA+'ipa_table.c'];cmd=source[IPA+'ipa_cmd.c']
    functions=mem.definition(table,r'^static void ipa_table_validate_build\(')+'\n'
    functions+='\n'.join(function(mem,cmd,n) for n in ['ipa_cmd_table_init_valid','ipa_cmd_table_init_add'])
    functions+='\n'.join(function(mem,table,n) for n in ['ipa_table_mem','ipa_table_hash_support','ipa_table_addr',
        'ipa_table_mem_valid','ipa_table_init_add','ipa_table_setup'])
    functions+='''
static __le64 table_memory[17];
static u32 table_allocations;
static void *dma_alloc_coherent(void *dev,size_t size,dma_addr_t *addr,int flags) {
 (void)dev;(void)flags;if(table_allocations++ || size!=sizeof(table_memory)) abort();
 memset(table_memory,0xa5,sizeof(table_memory));*addr=UINT64_C(0x80002000);return table_memory;
}
'''
    zero_size=re.search(r'^#define IPA_ZERO_RULE_SIZE[^\n]*',table,re.M)[0]
    functions=zero_size+'\n'+functions+function(mem,table,'ipa_table_init')
    c=c[:c.index('int main(void) {')]+functions+r'''
int main(void) {
 struct ipa i={.reg_virt=mmio,.gsi={.virt=mmio,.evt_ring={{.ring={.addr=UINT64_C(0x80003000),.count=256}}}},
  .mem=ipa_mem_local_data,.mem_count=ARRAY_SIZE(ipa_mem_local_data),.zero_addr=UINT64_C(0x80000000),
  .version=IPA_VERSION_4_2,.modem_route_count=8};
 for(u32 j=0;j<ARRAY_SIZE(ipa_gsi_endpoint_data);j++) {
  const struct ipa_gsi_endpoint_data *d=&ipa_gsi_endpoint_data[j];
  if(d->endpoint.filter_support)i.filtered|=UINT64_C(1)<<d->endpoint_id;
 }
 if(i.filtered!=0x12 || ipa_table_hash_support(&i)) abort();
 if(!ipa_table_mem_valid(&i,false) || !ipa_table_mem_valid(&i,true)) abort();
 if(i.route_count!=15 || i.filter_count!=14 || ipa_table_init(&i)) abort();
 printf("table %08llx ",(unsigned long long)i.table_addr);dump_bytes(i.table_virt,sizeof(table_memory));printf("\n");
 const struct reg *r=ipa_reg(&i,LOCAL_PKT_PROC_CNTXT);
 if(reg_bit(gsi_reg(&i.gsi,GSI_STATUS),ENABLED)!=1 || reg_field_max(r,IPA_BASE_ADDR)!=0x1ffff ||
    reg_decode(r,IPA_BASE_ADDR,0x5f0)!=0x5f0) abort();
 if(ipa_table_setup(&i) || allocations!=6 || commits!=1 || writes!=2) abort();
}
'''
    return c

def main():
    OUT.mkdir(mode=0o700,parents=True,exist_ok=True)
    mem=module('table_mem_reference','test-native-ipa-linux-memsetup.py');g=module('table_gsi_reference','test-native-ipa-linux-gsi.py')
    assert sha(g.ARCHIVE)==g.ARCHIVE_HASH
    inputs=mem.INPUTS+[IPA+n for n in ['ipa_table.c','ipa_table.h','ipa_version.h','ipa_endpoint.h','ipa_data.h']]
    source={}
    with tarfile.open(g.ARCHIVE) as t:
        prefix=t.getnames()[0].split('/')[0]+'/'
        for rel in inputs:
            b=t.extractfile(prefix+rel).read();p=OUT/'linux-source'/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b);source[rel]=b.decode()
    p=OUT/'linux-tables-oracle.c';p.write_text(oracle(source,mem,g));cc=shutil.which('clang') or shutil.which('cc');assert cc
    run([cc,'-std=gnu11','-Wall','-Wextra','-Werror',str(p),'-o',str(OUT/'linux-tables-oracle')],'c-build')
    golden=OUT/'linux-tables.txt';golden.write_text(run([str(OUT/'linux-tables-oracle')],'c-oracle').stdout)
    env=dict(os.environ,SUNFISH_TABLES_C_REFERENCE=str(golden));rustc=shutil.which('rustc');assert rustc
    binary=OUT/'tables-test';test=ROOT/'tests/sunfish_ipa_linux_tables_test.rs'
    run([rustc,'--edition=2024','-D','warnings','--test',str(test),'-o',str(binary)],'rust-build',env)
    tested=run([str(binary)],'rust-tests',env);groups=int(re.search(r'test result: ok\. (\d+) passed;',tested.stdout)[1])
    names=['sunfish_ipa_linux_config','sunfish_ipa_linux_memory','sunfish_ipa_linux_buffers','sunfish_ipa_linux_memsetup','sunfish_ipa_linux_tables']
    wrapper=OUT/'core.rs';wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names))
    run([rustc,'--edition=2024','-D','warnings','--crate-type=lib','--target=aarch64-unknown-none',str(wrapper),'-o',str(OUT/'tables-aarch64.rlib')],'aarch64-core')
    original=(ROOT/'rust/src/sunfish_ipa_linux_tables.rs').read_text();harness=test.read_text()
    mutations={'missing-v4-filter-opcode':('("V4_FILTER", 3, true)','("V4_FILTER", 4, true)'),
        'filter-skips-bitmap':('if filter { 8 } else { 16 }','if filter { 16 } else { 16 }'),
        'wrong-rule-base':('table.iova().to_le_bytes()','(table.iova()+8).to_le_bytes()'),
        'wrong-ring-cursor':('FIRST_TRE: usize = 4','FIRST_TRE: usize = 3')}
    for name,(before,after) in mutations.items():
        assert original.count(before)==1,(name,before);mutant=OUT/(name+'.rs');mutant.write_text(original.replace(before,after));body=harness
        for n in names:body=body.replace('../rust/src/'+n+'.rs',str(mutant if n==names[-1] else ROOT/'rust/src'/(n+'.rs')))
        p=OUT/(name+'-test.rs');p.write_text(body);binary=OUT/(name+'-test')
        run([rustc,'--edition=2024','-D','warnings','--test','--crate-name','tables_mutation',str(p),'-o',str(binary)],name+'-build',env)
        run([str(binary),'--exact','tables_match_original_linux_allocation_six_commands_and_doorbells'],name,env,expected=101)
    selected=['rust/src/'+n+'.rs' for n in names]+['rust/src/lib.rs','tests/sunfish_ipa_linux_tables_test.rs',
        'platforms/sunfish/test-native-ipa-linux-tables.py','platforms/sunfish/test-native-ipa-linux-memsetup.py',
        'platforms/sunfish/test-native-ipa-linux-gsi.py','platforms/sunfish/test-modem-qmi.py']
    for n in selected:
        p=OUT/'source'/n;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/n,p)
    manifest={'kernel_commit':g.PIN,'archive_sha256':g.ARCHIVE_HASH,'reference_sha256':{n:sha(OUT/'linux-source'/n) for n in source},
        'source_sha256':{n:sha(ROOT/n) for n in selected},'behavioral_groups':groups,'compiled_c_table_bytes':136,
        'compiled_c_command_count':6,'compiled_c_payload_and_tre_bytes':288,'compiled_c_doorbells':2,
        'encoder_mutations_rejected':len(mutations),'aarch64_no_std':True,'hardware_commands':[],
        'limits':'Offline table encoder/parser only; no native table transfer or readiness.'}
    manifest['artifacts']={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)} for p in OUT.iterdir() if p.is_file() and p.name!='manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'PASS: {groups} table groups, compiled original allocation/setup, 6 commands / 288 payload+TRE bytes, {len(mutations)} rejected mutations, AArch64 no_std; {OUT}')
if __name__=='__main__':main()
