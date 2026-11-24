"""Render retained exact predecessor FIFO/grant/ELF observations without new I/O."""
from pathlib import Path
import json,os,subprocess,tempfile,unittest
ROOT=Path(__file__).resolve().parents[1]
class RegistryReceiptPanel(unittest.TestCase):
 def test_actual_renderer(self):
  source=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text();a=source.index('fn time_from_us(');end=source.index('{',a)+1;depth=1
  while depth:depth+=(source[end]=='{')-(source[end]=='}');end+=1
  code=r'''
use std::cell::RefCell;
#[path=DISPLAY_PATH]mod physical;
struct Display{real:physical::Display,values:RefCell<[u32;30]>}
impl Display{fn replace_line(&self,n:usize,s:&str){assert!(n<30);if n==1{assert_eq!(s,"REGISTRY TX3 RECEIPT");assert!(s.len()<=24);}self.real.replace_line(n,s);}fn hex(&self,n:usize,s:&str,v:u32){assert!(n<30);assert!(s.len()<=15);self.values.borrow_mut()[n]=v;self.real.hex(n,s,v);}}
#[derive(Clone,Copy)]struct Grant{intent:u32,converted_at:Option<u64>,advertised_at:Option<u64>,ack_at:Option<u64>}
#[derive(Clone,Copy)]struct Prior{queued_at:Option<u64>,published_at:Option<u64>,done_at:Option<u64>,grant:Option<Grant>,publication:Option<Publication>}
#[derive(Clone,Copy)]struct Four{queued:Option<u64>,published:Option<u64>,done:Option<u64>}
#[derive(Clone,Copy)]struct Publication{transaction:u16,bytes:u16}
#[derive(Clone,Copy)]struct Pressure{reason:u8,used_slots:u8,unpublished_slots:u8,glink_wait_slots:u8,qrtr_wait_slots:u8,peer_flags:u8,peer_publication:Option<Publication>}
#[derive(Clone,Copy)]struct Dispatch{receive_pass_end:Option<u64>,registry_entered:Option<u64>,selected:Option<u64>}
#[derive(Clone,Copy)]struct Elf{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,dispatch:Dispatch,last_pressure:Option<Pressure>,waits_by_reason:[u32;6]}
#[derive(Clone,Copy)]struct Registry{txn4:Option<Four>,elf:Option<Elf>}
#[derive(Clone,Copy)]struct Window{predecessor:Option<Prior>}
#[derive(Clone,Copy)]struct Bounds{last_absent:Option<u64>,first_visible:Option<u64>,bytes_ahead:Option<u32>}
#[derive(Clone,Copy)]struct Fifo{completion:Option<Bounds>,peer_tail_at:Option<u64>,finished:bool,history_evictions:u32}
#[derive(Clone,Copy)]struct ModemOwner{registry:Registry,window:Window,fifo:Option<Fifo>}
impl ModemOwner{fn service_registry_trace(&self)->Registry{self.registry}fn registry_window_trace(&self)->Window{self.window}fn registry_reply_fifo(&self)->Option<Fifo>{self.fifo}fn tftp_wire_counts(&self)->[u32;4]{[0;4]}}
struct Paint{deferred:u32}struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;8]{[0;8]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,ipa_complete_seen:bool,registry_paint:Paint,wlan_query:Query}
impl RuntimeDomain{CLOCK_SOURCE}
#[path=PANEL_PATH]mod actual;
fn main(){
 let mut pixels=vec![0x00122b52u32;2*1080*2340];let d=Display{real:unsafe{physical::Display::new(pixels.as_mut_ptr())},values:RefCell::new([0;30])};
 let mut r=RuntimeDomain{runtime_started:100,hz:1_000_000,error:0,ipa_complete_seen:true,registry_paint:Paint{deferred:8},wlan_query:Query};
 let mut o=ModemOwner{window:Window{predecessor:Some(Prior{queued_at:Some(110),published_at:Some(120),done_at:Some(150),grant:Some(Grant{intent:42,converted_at:Some(115),advertised_at:Some(122),ack_at:Some(124)}),publication:Some(Publication{transaction:3,bytes:224})})},registry:Registry{txn4:Some(Four{queued:Some(130),published:Some(140),done:Some(180)}),elf:Some(Elf{received:Some(125),queued:Some(170),published:Some(190),done:Some(200),dispatch:Dispatch{receive_pass_end:Some(135),registry_entered:Some(145),selected:Some(155)},last_pressure:Some(Pressure{reason:1,used_slots:3,unpublished_slots:1,glink_wait_slots:2,qrtr_wait_slots:0,peer_flags:11,peer_publication:Some(Publication{transaction:4,bytes:224})}),waits_by_reason:[0,3,2,0,4,0]})},fifo:Some(Fifo{peer_tail_at:Some(125),finished:true,history_evictions:0,completion:Some(Bounds{last_absent:Some(141),first_visible:Some(149),bytes_ahead:Some(96)})})};
 d.replace_line(0,"NATIVE MPSS GLINK QRTR");r.show_tftp_completion(&d,&o);d.replace_line(29,"RESTART WHEN DONE");
 assert_eq!(&d.values.borrow()[2..29],&[0,10,20,25,41,49,96,50,0,15,22,24,40,80,25,55,70,90,100,3,224,1,8,42,1,0,0]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_TX3_RECEIPT_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 let original=o;o.window.predecessor.as_mut().unwrap().grant=None;o.window.predecessor.as_mut().unwrap().publication=None;r.show_tftp_completion(&d,&o);for row in [11,12,13,22,25]{assert_eq!(d.values.borrow()[row],u32::MAX);}
 o=original;let fifo=o.fifo.as_mut().unwrap();fifo.finished=false;fifo.completion=None;fifo.history_evictions=7;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[23],0);assert_eq!(d.values.borrow()[10],7);for row in [6,7,8]{assert_eq!(d.values.borrow()[row],u32::MAX);}assert_eq!(d.values.borrow()[5],25);
 o=original;o.window.predecessor=None;o.registry.txn4=None;o.registry.elf=None;o.fifo=None;r.show_tftp_completion(&d,&o);for row in 3..=23{assert_eq!(d.values.borrow()[row],u32::MAX,"absent row{row}");}assert_eq!(d.values.borrow()[24],8);assert_eq!(d.values.borrow()[25],u32::MAX);
 r.hz=0;r.show_tftp_completion(&d,&original);for row in [3,4,5,6,7,9,11,12,13,14,15,16,17,18,19,20]{assert_eq!(d.values.borrow()[row],u32::MAX);}assert_eq!(d.values.borrow()[22],224);assert_eq!(d.values.borrow()[23],1);assert_eq!(d.values.borrow()[25],42);
 println!("All27fields, exact predecessor/grant/receipt and ELF labels, incomplete/lost history, unknown grant/size/clocks and both framebuffers passed");
}
'''.replace('DISPLAY_PATH',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK_SOURCE',source[a:end]).replace('PANEL_PATH',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_tx3_receipt.rs')))
  with tempfile.TemporaryDirectory() as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code);subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True);subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
