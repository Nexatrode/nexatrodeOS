#!/usr/bin/env python3
"""Offline bounded RX queue versus pinned original Linux event/commit paths.

Allocator, DMA memory, transaction maps, completion queues and MMIO are host
stand-ins. No device, IRQ, native mapping, packet read or Ready capability.
"""
from pathlib import Path
import hashlib, importlib.util, json, os, re, shutil, subprocess, tarfile

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
OUT = ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-receive-events'
IPA = 'drivers/net/ipa/'


def module(name, file):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args, name, env=None):
    p = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True)
    (OUT / (name + '.log')).write_text(p.stdout + p.stderr)
    if p.returncode:
        raise SystemExit(f'{name}: exit {p.returncode}; see {OUT / (name + ".log")}')
    return p


def oracle(source, rx, mem, g, tables):
    # Reuse the source-derived initial 251 buffer/TRE model. Extend only its
    # host stand-ins; event parsing/update and ring/commit functions below
    # are original pinned definitions, compiled without semantic edits.
    c = rx.receive_oracle(source, mem, g, tables).split('int main(void){', 1)[0]
    old = 'struct gsi {void *dev,*virt;struct gsi_channel channel[3];};'
    assert c.count(old) == 1
    c = c.replace(old, '''struct gsi_evt_ring {struct gsi_ring ring;};
struct gsi {void *dev,*virt;struct gsi_channel channel[3];struct gsi_evt_ring evt_ring[2];};''')
    c = c.replace('static u8 mmio[0x20000];', 'static bool trace;\nstatic u8 mmio[0x20000];')
    old = 'printf("write %05zx %08x\\n",o,v);'
    assert c.count(old) == 1
    c = c.replace(old, 'if(trace)' + old)
    c = c.replace('static struct page pages[256];', 'static struct gsi_trans *map[256];\nstatic struct page pages[256];')
    old = 'static void gsi_trans_map(struct gsi_trans *t,u32 index){if(t->used_count!=1||index!=commits)abort();}'
    assert c.count(old) == 1
    c = c.replace(old, '''static void gsi_trans_map(struct gsi_trans *t,u32 index){
 if(t->used_count!=1||index!=commits||map[index%256])abort();map[index%256]=t;
}''')
    c += '''
#define le64_to_cpu(v) ((u64)(v))
#define __le16_to_cpu(v) ((u16)(v))
static struct gsi_trans *completed[251];static u32 completed_count;
static struct gsi_trans *gsi_channel_trans_mapped(struct gsi_channel *ch,u32 index){
 if(ch->id!=2||index>=256)abort();return map[index];
}
static void gsi_trans_tx_completed(struct gsi_trans *t){(void)t;abort();}
static void gsi_trans_move_complete(struct gsi_trans *t){
 if(completed_count>=251)abort();completed[completed_count++]=t;
 for(u32 j=0;j<256;j++)if(map[j]==t){map[j]=NULL;return;}abort();
}
'''
    # The reused table harness already includes the original event layout.
    assert c.count(mem.definition(source[IPA + 'gsi.c'], r'^struct gsi_event\s*\{')) == 1
    for name in ['gsi_ring_index', 'gsi_evt_ring_doorbell', 'gsi_event_trans',
                 'gsi_evt_ring_update', 'gsi_channel_update']:
        c += rx.fn(mem, source[IPA + 'gsi.c'], name)
    c += r'''
static void hex(const u8 *p){for(u32 j=0;j<16;j++)printf("%02x",p[j]);puts("");}
int main(void){
 struct ipa i={.version=IPA_VERSION_4_2,.reg_virt=mmio,.endpoint_count=17,.available={0xf72}};
 struct gsi *g=&i.gsi;g->virt=mmio;
 if(sizeof(struct gsi_event)!=16 || offsetof(struct gsi_event,len)!=8 ||
    offsetof(struct gsi_event,code)!=11 || offsetof(struct gsi_event,type)!=14 ||
    offsetof(struct gsi_event,chid)!=15)abort();
 const struct ipa_gsi_endpoint_data *d=&ipa_gsi_endpoint_data[IPA_ENDPOINT_AP_LAN_RX];
 struct gsi_channel *ch=&g->channel[d->channel_id];
 ch->gsi=g;ch->id=d->channel_id;ch->tre_count=d->channel.tre_count;ch->trans_tre_max=d->channel.tlv_count;
 ch->tre_ring=(struct gsi_ring){.addr=UINT64_C(0x80009000),.virt=tres,.count=ch->tre_count};
 ch->trans_info.tre_avail=gsi_channel_tre_max(g,ch->id);
 struct ipa_endpoint e={.ipa=&i,.endpoint_id=d->endpoint_id,.channel_id=d->channel_id,.config=d->endpoint.config};
 if(reg_field_max(ipa_reg(&i,ROUTE),ROUTE_DEF_PIPE)!=31 ||
    reg_decode(ipa_reg(&i,ROUTE),ROUTE_DEF_PIPE,0x01100050)!=8){dev_err(NULL,"bad RX reference geometry");abort();}
 ipa_endpoint_replenish_enable(&e);
 if(allocations!=251||transactions!=251||commits!=251||mapped!=251||doorbells!=16)abort();
 struct gsi_event events[256]={0};
 g->evt_ring[1].ring=(struct gsi_ring){.addr=UINT64_C(0x80008000),.virt=events,.count=256};
 ch->evt_ring_id=1;
 trace=true;
 const u32 counts[]={1,17,233,5,64,251,0,2,249,7,251,31,220,251,1,251,251,251,0};
 u32 total=0;
 for(u32 b=0;b<sizeof(counts)/sizeof(counts[0]);b++){
  u32 count=counts[b],next=(total+count)%256;
  printf("batch %u %08x\n",count,0x80008000+16*next);
  for(u32 k=0;k<count;k++){
   u32 n=total+k,slot=n%256;
   u16 len=n%17==0?0:n%19==0?8128:(n*73+19)%8129;
   events[slot]=(struct gsi_event){.xfer_ptr=UINT64_C(0x80009000)+16*slot,
    .len=len,.reserved1=n*3,.code=n*17,.reserved2=n*37,.type=n*29,.chid=2};
   printf("event %05x ",0x8000+16*slot);hex((u8*)&events[slot]);
  }
  values[0x1d090/4]=0x80008000+16*next;
  completed_count=0;gsi_channel_update(ch);
  if(completed_count!=count)abort();
  for(u32 k=0;k<count;k++){
   struct gsi_trans *t=completed[k];struct page *p=t->data;
   struct gsi_event *v=&events[(total+k)%256];
   printf("completed %zu %016llx %u %u %u\n",(size_t)(p-pages),(unsigned long long)v->xfer_ptr,t->len,v->code,v->type);
   /* Host consumes before reusing the same buffer. The native design uses
    * an explicit immediate-flush commit instead of delayed replenish work.
    */
   if(t->direction!=DMA_FROM_DEVICE||p!=&pages[(total+k)%251])abort();
   u32 index=ch->tre_ring.index%256;
   __gsi_trans_commit(t,true);
   printf("repost %05x ",0x9000+16*index);hex((u8*)&tres[index]);
  }
  total+=count;
 }
 printf("total %u\n",total);
}
'''
    old = 'u32 id,tre_count,trans_tre_max;'
    assert c.count(old) == 1
    c = c.replace(old, 'u32 id,tre_count,trans_tre_max,evt_ring_id;')
    return c


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    rx = module('events_rx', 'test-native-ipa-linux-receive.py')
    mem = module('events_mem', 'test-native-ipa-linux-memsetup.py')
    g = module('events_gsi', 'test-native-ipa-linux-gsi.py')
    tables = module('events_tables', 'test-native-ipa-linux-tables.py')
    assert sha(g.ARCHIVE) == g.ARCHIVE_HASH
    inputs = list(dict.fromkeys([*mem.INPUTS, IPA+'ipa_version.h', IPA+'ipa_endpoint.h',
                                IPA+'ipa_endpoint.c', IPA+'ipa_data.h', IPA+'ipa_table.c', IPA+'ipa_interrupt.c']))
    source = {}
    with tarfile.open(g.ARCHIVE) as tf:
        prefix = tf.getnames()[0].split('/')[0] + '/'
        for rel in inputs:
            data = tf.extractfile(prefix+rel).read()
            dest = OUT/'linux-source'/rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
            source[rel] = data.decode()
    # The only raw fields used by the original RX completion path are the
    # channel, TRE and length. Other bytes are preserved diagnostically.
    orig = source[IPA+'gsi.c']
    event_body = rx.fn(mem, orig, 'gsi_event_trans') + rx.fn(mem, orig, 'gsi_evt_ring_update')
    assert 'event->chid' in event_body and 'event->xfer_ptr' in event_body and 'event->len' in event_body
    assert not re.search(r'event->(?:code|type|reserved[12])', event_body)
    oracle_path = OUT/'linux-receive-events.c'
    oracle_path.write_text(oracle(source, rx, mem, g, tables))
    cc = shutil.which('clang') or shutil.which('cc')
    assert cc
    run([cc, '-std=gnu11', '-Wall', '-Wextra', '-Werror', str(oracle_path), '-o', str(OUT/'linux-receive-events')], 'c-build')
    golden = OUT/'linux-receive-events.txt'
    golden.write_text(run([str(OUT/'linux-receive-events')], 'c-oracle').stdout)
    env = dict(os.environ, SUNFISH_RECEIVE_EVENTS_C=str(golden))
    rustc = shutil.which('rustc')
    binary = OUT/'receive-events-test'
    run([rustc, '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2', '--test',
         str(ROOT/'tests/sunfish_ipa_linux_receive_events_test.rs'), '-o', str(binary)], 'rust-build', env)
    tested = run([str(binary), 'rxevents_'], 'rust-tests', env)
    groups = int(re.search(r'test result: ok\. (\d+) passed;', tested.stdout)[1])
    original = (ROOT/'rust/src/sunfish_ipa_linux_receive_events.rs').read_text()
    mutations = {
        'next-event-credit': ('(last * ELEMENT) as u64', '(next * ELEMENT) as u64'),
        'allow-oversize-length': ('if length > rx::DMA_BYTES {', 'if length > u16::MAX as usize {'),
        'truncate-transfer-pointer': ('if ptr != self.tre_base +', 'if (ptr as u32 as u64) != self.tre_base +'),
        'overwrite-old-tre': ('self.tre_offset + tail * ELEMENT', 'self.tre_offset + self.report.completion_cursor * ELEMENT'),
        'guess-raw-code': ('if raw[15] != rx::CHANNEL {', 'if raw[11] != 2 || raw[15] != rx::CHANNEL {'),
        'omit-descriptor-stores': ('if pass == 0 {', 'if pass == 99 {'),
    }
    harness = (ROOT/'tests/sunfish_ipa_linux_receive_events_test.rs').read_text().replace('../rust/src/', str(ROOT/'rust/src')+'/')
    for label, (before, after) in mutations.items():
        assert original.count(before) == 1, before
        mutant = OUT/(label+'.rs')
        mutant.write_text(original.replace(before, after))
        test = OUT/(label+'-test.rs')
        test.write_text(harness.replace(str(ROOT/'rust/src/sunfish_ipa_linux_receive_events.rs'), str(mutant)))
        target = OUT/(label+'-test')
        # An unused binding in the altered credit formula is diagnostic only;
        # the mutant must compile, then fail behavior/C comparison at runtime.
        run([rustc, '--edition=2024', '-A', 'unused_variables', '-C', 'opt-level=2', '--test', str(test), '-o', str(target)], label+'-build', env)
        result = subprocess.run([str(target), 'rxevents_'], cwd=ROOT, env=env, capture_output=True, text=True)
        (OUT/(label+'.log')).write_text(result.stdout+result.stderr)
        assert result.returncode != 0, label+' survived'
    names = ['sunfish_ipa_linux_buffers', 'sunfish_ipa_linux_gsi', 'sunfish_ipa_linux_endpoint',
             'sunfish_ipa_linux_receive', 'sunfish_ipa_linux_receive_events']
    wrapper = OUT/'core.rs'
    wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names))
    run([rustc, '--edition=2024', '-D', 'warnings', '--crate-type=lib', '--target=aarch64-unknown-none',
         str(wrapper), '-o', str(OUT/'receive-events-aarch64.rlib')], 'aarch64-core')
    selected = ['rust/src/'+n+'.rs' for n in names]+[
        'tests/sunfish_ipa_linux_receive_events_test.rs', 'platforms/sunfish/test-native-ipa-linux-receive-events.py',
        'platforms/sunfish/test-native-ipa-linux-receive.py', 'docs/sunfish-ipa-receive-plan.md']
    for rel in selected:
        dest = OUT/'source'/rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT/rel, dest)
    manifest = {'source_sha256': {rel: sha(ROOT/rel) for rel in selected},
        'linux_archive_sha256': g.ARCHIVE_HASH, 'linux_source_sha256': {rel: hashlib.sha256(v.encode()).hexdigest() for rel,v in source.items()},
        'behavioral_groups': groups, 'mutations_rejected': list(mutations),
        'compiled_linux_events': int(re.search(r'^total (\d+)$', golden.read_text(), re.M)[1]),
        'aarch64_no_std': True, 'hardware_commands': [], 'native_backend': False, 'library_exported': False,
        'scope': 'Bounded event and buffer-recycling core with compiled original Linux host comparison; no physical RX, packet-read authority or readiness.'}
    manifest['artifacts'] = {f.name: {'bytes': f.stat().st_size, 'sha256': sha(f)} for f in OUT.iterdir() if f.is_file() and f.name != 'manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    print(f'PASS: {groups} RX event/recycling groups, {manifest["compiled_linux_events"]} original Linux events, {len(mutations)} rejected mutations, AArch64 no_std; {OUT}')


if __name__ == '__main__':
    main()
