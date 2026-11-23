"""Render all 27 post-reply fields with the real framebuffer and clock conversion."""
from pathlib import Path
import json, os, subprocess, tempfile, unittest
ROOT=Path(__file__).resolve().parents[1]
class RegistryPostReplyPanel(unittest.TestCase):
 def test_actual_renderer(self):
  source=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text();a=source.index('fn time_from_us(');end=source.index('{',a)+1;depth=1
  while depth:depth+=(source[end]=='{')-(source[end]=='}');end+=1
  code=r'''
use std::cell::RefCell;
#[path=DISPLAY]mod physical;
struct Display{real:physical::Display,values:RefCell<[u32;30]>}
impl Display{fn replace_line(&self,n:usize,s:&str){assert!(n<30);if n==1{assert_eq!(s,"REGISTRY POST REPLY");assert!(s.len()<=24);}self.real.replace_line(n,s);}fn hex(&self,n:usize,s:&str,v:u32){assert!(n<30);self.values.borrow_mut()[n]=v;self.real.hex(n,s,v);}}
#[derive(Clone,Copy)]struct Grant{intent:u32,converted_at:Option<u64>,advertised_at:Option<u64>,ack_at:Option<u64>}
#[derive(Clone,Copy)]struct Prior{published_at:Option<u64>,done_at:Option<u64>,grant:Option<Grant>}
#[derive(Clone,Copy)]struct Pressure{calls:u32,mask:u32,grant_gates:[u32;12]}
#[derive(Clone,Copy)]struct Pair{publication:Option<Pressure>,publication_frozen:bool}
#[derive(Clone,Copy)]struct Window{predecessor:Option<Prior>,target:Option<Pair>}
#[derive(Clone,Copy)]struct Bounds{last_absent:Option<u64>,first_visible:Option<u64>,bytes_ahead:Option<u32>}
#[derive(Clone,Copy)]struct Fifo{peer_tail_at:Option<u64>,completion:Option<Bounds>,finished:bool,history_evictions:u32}
#[derive(Clone,Copy)]struct Tx4{queued:Option<u64>,published:Option<u64>}
#[derive(Clone,Copy)]struct Elf{done:Option<u64>}
struct Registry{txn4:Option<Tx4>,elf:Option<Elf>}
#[derive(Clone,Copy)]struct ModemOwner{window:Window,fifo:Option<Fifo>,tx4:Option<Tx4>,elf:Option<Elf>}
impl ModemOwner{fn registry_window_trace(&self)->Window{self.window}fn registry_reply_fifo(&self)->Option<Fifo>{self.fifo}fn service_registry_trace(&self)->Registry{Registry{txn4:self.tx4,elf:self.elf}}fn tftp_wire_counts(&self)->[u32;4]{[0,0,2,0]}}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;8]{[0,0,0,0,0,0,0,3]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,ipa_complete_seen:bool,wlan_query:Query}
impl RuntimeDomain{CLOCK}
#[path=PANEL]mod actual;
fn main(){
 let mut pixels=vec![0x00122b52u32;2*1080*2340];let real=unsafe{physical::Display::new(pixels.as_mut_ptr())};let d=Display{real,values:RefCell::new([0;30])};
 let mut r=RuntimeDomain{runtime_started:100,hz:1_000_000,error:0,ipa_complete_seen:true,wlan_query:Query};
 let mut o=ModemOwner{window:Window{predecessor:Some(Prior{published_at:Some(200),done_at:Some(800),grant:Some(Grant{intent:27,converted_at:Some(150),advertised_at:Some(300),ack_at:Some(400)})}),target:Some(Pair{publication:Some(Pressure{calls:77,mask:0x1234,grant_gates:[1,2,3,4,5,6,7,8,9,10,11,12]}),publication_frozen:true})},fifo:Some(Fifo{peer_tail_at:Some(250),completion:Some(Bounds{last_absent:Some(600),first_visible:Some(700),bytes_ahead:Some(72)}),finished:true,history_evictions:2}),tx4:Some(Tx4{queued:Some(450),published:Some(750)}),elf:Some(Elf{done:Some(900)})};
 d.replace_line(0,"NATIVE MPSS GLINK QRTR");r.show_tftp_completion(&d,&o);d.replace_line(29,"RESTART WHEN DONE");
 assert_eq!(&d.values.borrow()[2..29],&[0,100,150,500,600,700,72,0x10002,27,50,200,300,350,650,2,77,0x1234,0x10002,0x30004,0x50006,0x70008,0x9000a,0xb000c,800,1,2,3]);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_POST_REPLY_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 let original=o;o.window.target.as_mut().unwrap().publication_frozen=false;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[16],1);
 o=original;o.window.target.as_mut().unwrap().publication.as_mut().unwrap().grant_gates=[u32::MAX;12];o.fifo.as_mut().unwrap().history_evictions=u32::MAX;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[9],0x1ffff);for row in 19..=24{assert_eq!(d.values.borrow()[row],u32::MAX);}
 o=original;o.fifo.as_mut().unwrap().completion=Some(Bounds{last_absent:None,first_visible:None,bytes_ahead:None});r.show_tftp_completion(&d,&o);for row in [5,6,8]{assert_eq!(d.values.borrow()[row],u32::MAX);}
 o=original;o.fifo=None;o.window.predecessor=None;o.window.target=None;o.tx4=None;o.elf=None;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[16],0);for row in (3..=25).filter(|row|*row!=16){assert_eq!(d.values.borrow()[row],u32::MAX,"absent row{row}");}
 r.hz=0;r.show_tftp_completion(&d,&original);for row in [3,4,5,6,7,11,12,13,14,15,25]{assert_eq!(d.values.borrow()[row],u32::MAX);}assert_eq!(d.values.borrow()[17],77);
 println!("All27rows, frozen/live/missing, saturated pairs, evicted visibility, clocks and both framebuffers passed");
}
'''.replace('path=DISPLAY', 'path='+json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',source[a:end]).replace('path=PANEL', 'path='+json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_post_reply.rs')))
  with tempfile.TemporaryDirectory() as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
