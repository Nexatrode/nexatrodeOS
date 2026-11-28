from pathlib import Path
import json,os,subprocess,tempfile,unittest
from sunfish_registry_elf_pressure_panel_test import pressure_fixture_code
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
def receipt_fixture_code():
  code=pressure_fixture_code()
  code=code.replace('struct Prior{','struct Prior{grant:Option<Grant>,').replace('Prior{queued_at:','Prior{grant:None,queued_at:')
  code=code.replace('struct ModemOwner{','struct ModemOwner{tx3_fifo:Option<Fifo>,elf_fifo:Option<ElfFifo>,')
  code=code.replace('impl ModemOwner{','impl ModemOwner{fn registry_reply_fifo(&self)->Option<Fifo>{self.tx3_fifo}fn elf_reply_fifo(&self)->Option<ElfFifo>{self.elf_fifo}')
  code=code.replace('#[path=PANEL]mod panel;',r'''
#[derive(Clone,Copy,Default)]struct Grant{converted_at:Option<u64>,advertised_at:Option<u64>,ack_at:Option<u64>,intent:u32}
#[derive(Clone,Copy,Default)]struct Bounds{last_absent:Option<u64>,first_visible:Option<u64>,bytes_ahead:Option<u32>}
#[derive(Clone,Copy,Default)]struct Fifo{peer_tail_at:Option<u64>,completion:Option<Bounds>,finished:bool,history_evictions:u32}
#[derive(Clone,Copy,Default)]struct ElfFifo{remote_intent:u32,armed_at:Option<u64>,unread_at_arm:u32,core_done_at:Option<u64>,fifo:Fifo}
#[path=PANEL]mod panel;
''')
  return code

class Panel(unittest.TestCase):
 def test_actual_receipt_panel(self):
  code=receipt_fixture_code()
  code+=r'''
 o.trace.queued_names=0x75;o.trace.completed_names=0x75;
 o.tx3_fifo=Some(Fifo{peer_tail_at:Some(140),completion:Some(Bounds{last_absent:Some(180),first_visible:Some(200),bytes_ahead:Some(96)}),finished:true,history_evictions:24});
 o.elf_fifo=Some(ElfFifo{remote_intent:7,armed_at:Some(321),unread_at_arm:0,core_done_at:Some(390),fifo:Fifo{peer_tail_at:Some(330),completion:Some(Bounds{last_absent:Some(350),first_visible:Some(370),bytes_ahead:Some(8)}),finished:true,history_evictions:0}});
 o.window.predecessor.as_mut().unwrap().grant=Some(Grant{converted_at:Some(125),advertised_at:Some(150),ack_at:Some(170),intent:42});
 let values=|n|d.rows.borrow().iter().find(|(i,_)|*i==n).unwrap().1.clone();
 r.show_registry_receipt_fifo(&d,&o);
 assert_eq!(*d.title.borrow(),"REGISTRY RECEIPT FIFO");assert_eq!(d.rows.borrow().len(),27);
 assert_eq!(values(3),[20,30,40,110,120,130]);assert_eq!(values(4),[200,210,220,230,0x75,0x75]);
 assert_eq!(values(6),[40,80,100,96,1,24]);assert_eq!(values(7),[230,250,270,8,1,0]);assert_eq!(values(8),[221,0,7,290,230]);assert_eq!(values(9),[25,50,70,42]);
 assert_eq!(values(10),[0;6]);assert_eq!(values(14),[0,1,9,0,0,0]);assert_eq!(values(18),[0,1,2,3,4,5]);assert_eq!(values(19),[240,680,11,3,400,900]);
 assert_eq!(values(22),[0x4e,0xbf,820,850,860,0x50090013]);assert_eq!(values(23),[0x4f,0x8f,u32::MAX,u32::MAX,890,0x500a0013]);assert_eq!(values(24),[33,1,34,1,3,1]);assert_eq!(values(26),[0xcf,0x35,3,1,21,0x11002e]);assert_eq!(values(28),[1,0x7fffffff,0,0,1,0]);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_RECEIPT_FIFO_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 // An unfinished or evicted history cannot invent an RX_DONE bound.
 o.elf_fifo.as_mut().unwrap().fifo=Fifo{history_evictions:50,..Fifo::default()};o.elf_fifo.as_mut().unwrap().core_done_at=None;
 d.rows.borrow_mut().clear();r.show_registry_receipt_fifo(&d,&o);assert_eq!(values(7),[u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,50]);assert_eq!(values(8)[3],u32::MAX);
 r.hz=0;d.rows.borrow_mut().clear();r.show_registry_receipt_fifo(&d,&o);assert_eq!(&values(6)[..3],&[u32::MAX;3]);assert_eq!(&values(6)[3..],&[96,1,24]);
 r.hz=1000000;o=ModemOwner::default();d.rows.borrow_mut().clear();r.show_registry_receipt_fifo(&d,&o);assert_eq!(values(6),[u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,u32::MAX]);assert_eq!(values(8),[u32::MAX;5]);assert_eq!(values(9),[u32::MAX;4]);assert_eq!(values(24),[u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,0]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual receipt panel passed: distinct TX3/ELF bounds, Core/Owner separation, missing/unfinished/clockless histories, exact digest identity, all rows and physical buffers");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_receipt_fifo.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
