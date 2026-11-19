#!/usr/bin/env python3
"""Bind native GSI ownership, closed MMIO and interrupt/counter sequencing."""
import argparse,hashlib,importlib.util,json,os,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
OUT=ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-gsi'

def check(entry,parent,core):
    required=["pub(super) const CHANNEL: usize = 3", "retained: IrqIo<'a>",
        "uc::Io::guard(&mut self.retained)", "let plan = state._gsi.as_ref()?.plan()",
        "state._gsi = Some(", "gsi::SetupOnce::new(plan, gsi::command_timeout_ticks(hz).unwrap())",
        "once.start(io)", "once.notified(io, now)", "once.timeout(io, now)", "once.external_fault(io)",
        "if badge != GSI_BADGE", "dispatch_uc(state, true)", "pool.quarantine()", "domain.stop_on_fault()",
        "sel4::Cap::<sel4::cap_type::Endpoint>::from_bits(1)", "sel4::Cap::<sel4::cap_type::Reply>::from_bits(4)",
        ".nb_recv(", "cntpct_el0", "cntfrq_el0", "1_000_000..=1_000_000_000",
        "self.retained.registers.gsi", "if !self.writable(w)", "if !self.readable(offset)",
        "gsi::CHANNEL_COMMAND | gsi::EVENT_COMMAND => value < 4", "[0x402, 0x422, 0x442, 0x462]",
        "Channel::new(CHANNEL)", '.write_volatile(w.value.to_le())', 'asm!("dsb sy"']
    for token in required:
        if token not in entry:raise ValueError('native GSI dependency changed: '+token)
    if entry.index('state._gsi = Some(')>entry.index('once.start(io)'):raise ValueError('GSI owner not retained before start')
    for token in ['mod pixel_ipa_gsi;', '_gsi: Option<personal_os_rust_server::sunfish_ipa_linux_gsi::SetupOnce>',
                  'pixel_ipa_gsi::start(&d, &mut state)', 'pixel_ipa_gsi::late_notification(self)']:
        if token not in parent:raise ValueError('native GSI integration changed: '+token)
    for token in ['self.started_at = io.ticks()', 'if pending != kind', 'if global != 2',
                  'self.read(io, self.report.operation.source())? != 0', 'matches!(result, 1 | 2)',
                  'pub fn late_notification(', 'io.acknowledge()?']:
        if token not in core:raise ValueError('native GSI core sequencing changed: '+token)
    # Pinned gsi_setup initializes ERROR_LOG; raw pre-initialization bits
    # alone must not act as an invented interrupt/readiness prerequisite.
    ordered=['self.report.error_log_before = Some(self.read(io, ERROR_LOG)?)',
             'self.require_quiet(io)?', 'self.write(io, Word::new(ERROR_LOG, 0))?',
             'self.report.error_log_after = Some(self.read(io, ERROR_LOG)?)',
             'self.require_quiet(io)?']
    tail=core[core.index('pub fn start(&mut self, io: &mut impl SetupIo)'):]
    for token in ordered:
        if token not in tail:raise ValueError('GSI error-log initialization changed: '+token)
        tail=tail.split(token,1)[1]

def audit():
    s=importlib.util.spec_from_file_location('native_gsi_pas_audit',HERE/'audit-native-ipa-firmware-source.py')
    firmware=importlib.util.module_from_spec(s);s.loader.exec_module(firmware)
    result=firmware.audit()
    paths=['rust/src/pixel_ipa_gsi.rs','rust/src/sunfish_native_startup_preflight.rs','rust/src/sunfish_ipa_linux_gsi.rs']
    texts=[(ROOT/p).read_text()for p in paths];check(*texts)
    mutations=[(0,'value < 4','value < 256'),(0,'pub(super) const CHANNEL: usize = 3','pub(super) const CHANNEL: usize = 2'),
        (0,'uc::Io::guard(&mut self.retained)','Ok(())'),(0,'if badge != GSI_BADGE','if false'),
        (0,'once.timeout(io, now)','Ok(())'),(0,'state._gsi = Some(','let unretained = Some('),
        (2,'if global != 2','if global != 3'),
        (2,'self.write(io, Word::new(ERROR_LOG, 0))?', 'self.write(io, Word::new(ERROR_LOG, 1))?')]
    for index,old,new in mutations:
        assert texts[index].count(old)==1,old
        candidate=texts.copy();candidate[index]=candidate[index].replace(old,new)
        try:check(*candidate)
        except ValueError:continue
        raise ValueError('native GSI mutation accepted: '+old)
    # Compile the backend's actual two pure whitelist methods unchanged.
    # Linux's separately compiled writes must all pass; unrelated pages and
    # every non-ALLOCATE channel/generic encoding must fail.
    pure=texts[0].split("impl Native<'_> {",1)[1].split("impl gsi::Io for Native<'_>",1)[0]
    pure='impl Native {'+pure
    fixture=(ROOT/'tests/sunfish_ipa_linux_gsi_test.rs').read_text()
    for name in ['sunfish_ipa_linux_buffers','sunfish_ipa_linux_gsi']:
        fixture=fixture.replace('../rust/src/'+name+'.rs',str(ROOT/'rust/src'/(name+'.rs')))
    extra=r'''
use sunfish_ipa_linux_gsi as gsi;
struct Native { plan: gsi::Plan }
#[test]
fn native_whitelist_accepts_compiled_linux_and_rejects_unrelated_mmio(){
    let n=Native{plan:setup_plan()};
    for name in ["SUNFISH_GSI_C_SETUP_REFERENCE","SUNFISH_GSI_C_REFERENCE"] {
        for line in std::fs::read_to_string(std::env::var(name).unwrap()).unwrap().lines(){
            let parts:Vec<_>=line.split_whitespace().collect();
            let offset=usize::from_str_radix(parts[2],16).unwrap();let value=u32::from_str_radix(parts[3],16).unwrap();
            assert!(n.writable(gsi::Word{offset,value,readback_mask:u32::MAX}),"{offset:x}/{value:x}");
        }
    }
    for offset in (0..0x2c000).step_by(4) {
        if ![0xc000,0x1c000,0x1d000,0x1e000,0x1f000].contains(&(offset&!4095)) {
            for value in [0,1,0x1000002,0x80004000,u32::MAX] {
                assert!(!n.writable(gsi::Word{offset,value,readback_mask:u32::MAX}));
            }
        }
    }
    for value in 0..65536 {
        for offset in [gsi::CHANNEL_COMMAND,gsi::EVENT_COMMAND] {
            assert_eq!(n.writable(gsi::Word{offset,value,readback_mask:u32::MAX}),value<4);
        }
        assert_eq!(n.writable(gsi::Word{offset:gsi::GENERIC_COMMAND,value,readback_mask:u32::MAX}),[0x402,0x422,0x442,0x462].contains(&value));
    }
    for offset in [0,4,0x1e000,0x1e100,0x1f018,0x1f008,0x1f010,0x2bffc] {assert!(!n.readable(offset));}
    for offset in [gsi::GSI_STATUS,gsi::HW_PARAM_2,gsi::TYPE_STATUS,gsi::GLOBAL_STATUS,gsi::GENERIC_SCRATCH] {assert!(n.readable(offset));}
}
'''
    fixture+='\n'+pure+extra
    p=OUT/'native-whitelist-test.rs';p.write_text(fixture)
    env=dict(os.environ,SUNFISH_GSI_C_SETUP_REFERENCE=str(OUT/'linux-setup-writes.txt'),SUNFISH_GSI_C_REFERENCE=str(OUT/'linux-context-writes.txt'))
    binary=OUT/'native-whitelist-test'
    for name,args in [('native-whitelist-build',['rustc','--edition=2024','-D','warnings','--test',str(p),'-o',str(binary)]),
        ('native-whitelist',[str(binary),'--exact','native_whitelist_accepts_compiled_linux_and_rejects_unrelated_mmio'])]:
        v=subprocess.run(args,cwd=ROOT,env=env,text=True,capture_output=True);(OUT/(name+'.log')).write_text(v.stdout+v.stderr)
        if v.returncode:raise ValueError(name+' failed; see private log')
    result['native_gsi']={'source_sha256':dict(zip(paths,[hashlib.sha256(t.encode()).hexdigest()for t in texts])),
        'mutations_rejected':len(mutations),'compiled_native_whitelist':True,'kernel_irq':464,'microkit_channel':3,
        'counter':'cntpct_el0','timeout_ms':50,'poll_input_cap':1,'poll_reply_cap':4,
        'scope':'Native allocation/context backend; no channel START, TRE publish, MPSS/WLAN start or flash',
        'physical_gsi_delivery_proven':False,'functional_dma_proven':False}
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    a.output.write_text(json.dumps(audit(),indent=2)+'\n');print('Native GSI retained ownership, guarded MMIO, IRQ/counter path and compiled whitelist pass; 8 mutations rejected.')
