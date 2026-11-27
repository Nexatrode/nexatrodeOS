"""Both delayed phases remain visible on the actual physical framebuffer."""
from pathlib import Path
import json,os,subprocess,tempfile,unittest
from sunfish_elf_digest_panel_test import fixture_code
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
def pressure_fixture_code():
  code=fixture_code();code=code[:code.index(' let values=|n|')]
  code=code.replace('waits_by_reason:[u32;6]}','waits_by_reason:[u32;6],dispatch:Dispatch}').replace('waits_by_reason:[0,1,2,3,4,5]}','waits_by_reason:[0,1,2,3,4,5],..Timed::default()}')
  code=code.replace('struct Trace{elf:','struct Trace{queued_names:u32,completed_names:u32,elf:')
  code=code.replace('struct ElfWindow{writer:Writer}','struct ElfWindow{writer:Writer,last_failure:Option<Admission>}').replace('ElfWindow{writer}','ElfWindow{writer,..ElfWindow::default()}')
  code=code.replace('struct ModemOwner{elf_writer:','struct ModemOwner{retry:Retry,pool:Option<Pool>,completion_taken:bool,elf_writer:')
  code=code.replace('impl ModemOwner{','impl ModemOwner{fn elf_retry_trace(&self)->Retry{self.retry}fn elf_tx_slots(&self)->Option<&Pool>{self.pool.as_ref()}fn elf_completion_turn_taken(&self)->bool{self.completion_taken}fn elf_capacity_turn_trace(&self)->Capacity{Capacity{flags:15,result:1}}fn efs_overlap_trace(&self)->Pair{Pair{flags:0x7f,admitted:1,offers:1}}')
  code=code.replace('#[path=PANEL]mod panel;',r'''
#[derive(Clone,Copy,Default)]struct Turns{calls:u32,omitted:u32}
#[derive(Clone,Copy,Default)]struct Dispatch{receive_pass_end:Option<u64>,registry_entered:Option<u64>,selected:Option<u64>,turns:Turns}
#[derive(Clone,Copy,Default)]struct Entry{transaction:u16,flags:u8,kind:u8}
#[derive(Clone,Copy,Default)]struct Admission{at:Option<u64>,sequence:u32,reason:u8,count:u8,gates:u16,entries:[Option<Entry>;2]}
#[derive(Clone,Copy,Default)]struct Retry{receipt_at:Option<u64>,work_returned_at:Option<u64>,registry_entry_at:Option<u64>,registry_selected_at:Option<u64>,flags:u32,blocker_transaction:Option<u16>}
#[derive(Clone,Copy,Default)]struct Free{at:Option<u64>,slot:u8,cause:u8,empty_mask:u8}
#[derive(Default)]struct Pool{first_free:Option<Free>,admitted_slot:Option<u8>,admitted:Option<u64>}
struct Capacity{flags:u32,result:u8}struct Pair{flags:u32,admitted:u32,offers:u32}
#[path=PANEL]mod panel;
''')
  return code

class Panel(unittest.TestCase):
 def test_actual_unified_panel(self):
  code=pressure_fixture_code()
  code+=r'''
 o.trace.queued_names=0x75;o.trace.completed_names=0x75;
 {let p=o.window.predecessor.as_mut().unwrap().publication.as_mut().unwrap();p.calls=128;p.mask=0xe0ac;p.turn_gates=[0,9,105,13,0,0,0,1];p.grant_gates=[19,36,37,36,0,0,0,0,0,0,0,0];p.ack=PubAck{sent:37,blocked:0,done_ahead:32,ads_ahead:36};}
 o.trace.elf.as_mut().unwrap().dispatch=Dispatch{receive_pass_end:Some(301),registry_entered:Some(304),selected:Some(305),turns:Turns{calls:12,omitted:4}};
 o.elf_writer.as_mut().unwrap().last_failure=Some(Admission{at:Some(290),sequence:7,reason:1,count:2,gates:3,entries:[Some(Entry{transaction:3,flags:0x37,kind:4}),Some(Entry{transaction:4,flags:0x3b,kind:4})]});
 o.pool=Some(Pool{first_free:Some(Free{at:Some(308),slot:1,cause:1,empty_mask:2}),admitted_slot:Some(1),admitted:Some(310)});
 o.handoff=Handoff{flags:31,arms:2,receipts:1,services:1,result:1};o.completion_taken=true;o.retry=Retry{receipt_at:Some(390),work_returned_at:Some(400),registry_entry_at:Some(420),registry_selected_at:Some(430),flags:0x1ff,blocker_transaction:Some(4)};
 let values=|n|d.rows.borrow().iter().find(|(i,_)|*i==n).unwrap().1.clone();
 r.show_registry_elf_pressure(&d,&o);assert_eq!(*d.title.borrow(),"REGISTRY ELF PRESSURE");assert_eq!(d.rows.borrow().len(),27);
 assert_eq!(values(3),[20,30,40,110,120,130]);assert_eq!(values(4),[200,210,220,230,0x75,0x75]);assert_eq!(values(5),[2,128,0xe0ac,11,0xe0ac,1]);
 assert_eq!(values(6),[0,9,105,13,0,0]);assert_eq!(values(7),[0,1,19,36,37,36]);assert_eq!(values(8),[0;6]);assert_eq!(values(9),[0,0,37,0,32,36]);
 assert_eq!(values(10),[0,1,9,0,0,0]);assert_eq!(values(11),[0,1,0,1,2,3]);assert_eq!(values(12),[4,5,6,7,8,9]);assert_eq!(values(13),[10,11,9,0,1,1]);assert_eq!(values(14),[0,1,2,3,4,5]);assert_eq!(values(15),[201,204,205,210,12,4]);assert_eq!(values(16),[190,0x701,2,3,0x33704,0x43b04]);
 assert_eq!(values(17),[240,680,11,3,400,900]);assert_eq!(values(18),[211,700,31,3,21,208]);
 assert_eq!(values(19),[0x4e,0xbf,33,1,0x50001,0]);assert_eq!(values(20),[800,810,820,850,860,0x50090013]);assert_eq!(values(21),[0x4f,0x8f,34,1,0x50001,0]);assert_eq!(values(22),[870,880,u32::MAX,u32::MAX,890,0x500a0013]);
 assert_eq!(values(23),[31,2,1,1,1,1]);assert_eq!(values(24),[290,300,320,330,0x1ff,4]);assert_eq!(values(25),[0x10102,1,210,15,1]);assert_eq!(values(26),[0xcf,0x35,3,1,21,0x11002e]);assert_eq!(values(27),[3,1,2,2,0x1017f,1]);assert_eq!(values(28),[1,0x7fffffff,0,0,1,0]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_REGISTRY_ELF_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 // A clean ELF pass cannot hide the older slow TX3 snapshot.
 {let w=&mut o.elf_writer.as_mut().unwrap().writer;w.passes=1;w.timed=1;w.omitted=0;w.max_gap=None;w.publication.as_mut().unwrap().calls=1;w.publication.as_mut().unwrap().mask=0xa0;}
 d.rows.borrow_mut().clear();r.show_registry_elf_pressure(&d,&o);assert_eq!(values(5),[2,128,0xe0ac,1,0xa0,1]);assert_eq!(values(6),[0,9,105,13,0,0]);assert_eq!(values(17)[5],u32::MAX);
 // The same page always retains a clean TX3 and the slow queued ELF case.
 o.window.predecessor.as_mut().unwrap().publication.as_mut().unwrap().mask=0x3fa4;o.elf_writer.as_mut().unwrap().writer.publication.as_mut().unwrap().mask=0x1e24ac;
 r.hz=0;d.rows.borrow_mut().clear();r.show_registry_elf_pressure(&d,&o);assert_eq!(values(3),[u32::MAX;6]);assert_eq!(values(5),[2,128,0x3fa4,1,0x1e24ac,1]);assert_eq!(&values(20)[..5],&[u32::MAX;5]);assert_eq!(values(20)[5],0x50090013);
 r.hz=1000000;o.window.predecessor.as_mut().unwrap().queued_at=Some(90);d.rows.borrow_mut().clear();r.show_registry_elf_pressure(&d,&o);assert_eq!(values(3)[0],u32::MAX);
 o=ModemOwner::default();d.rows.borrow_mut().clear();r.show_registry_elf_pressure(&d,&o);assert_eq!(values(6),[u32::MAX;6]);assert_eq!(values(16),[u32::MAX;6]);assert_eq!(values(20),[u32::MAX;6]);assert_eq!(values(14),[0;6]);assert_eq!(values(23),[0;6]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual unified renderer passed: both frozen gate families, mixed/cold/clockless snapshots, exact digest IDs/cancel state, wait/blocker/retry/capacity observations, every row and both physical buffers");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_elf_pressure.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
