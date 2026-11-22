"""Exercise the actual stage renderer with missing and mismatched observations."""
from pathlib import Path
import json, os, subprocess, tempfile, unittest
ROOT=Path(__file__).resolve().parents[1]
class StagePanel(unittest.TestCase):
 def test_stage_boundaries_and_missing_identity(self):
  source=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(); a=source.index('fn time_from_us('); b=source.index('{',a); depth=1;end=b+1
  while depth:
   depth+=(source[end]=='{')-(source[end]=='}');end+=1
  code=r'''
use std::cell::RefCell;
#[path=DISPLAY_PATH] mod physical;
struct Display{real:physical::Display,values:RefCell<[u32;30]>}
impl Display{fn replace_line(&self,n:usize,s:&str){assert!(n<30);self.real.replace_line(n,s);}fn hex(&self,n:usize,s:&str,v:u32){assert!(n<30);self.values.borrow_mut()[n]=v;self.real.hex(n,s,v);}}
#[derive(Clone,Copy)]struct Address{node:u32,port:u32}
#[derive(Clone,Copy)]struct Stages{final_prepared:bool,final_queued:bool,dispatch_at:Option<u64>,prepared_at:Option<u64>,returned_at:Option<u64>,queued_at:Option<u64>}
#[derive(Clone,Copy)]struct Data{block:u16,received_at:Option<u64>,stages:Stages}
#[derive(Clone,Copy)]struct Ack{phase:u8,published_at:Option<u64>,done_at:Option<u64>}
#[derive(Clone,Copy)]struct Write{final_block:Option<u16>,peer:Address,data:Option<Data>,ack:Ack}
#[derive(Clone,Copy)]struct Turns{runs:[u32;4],omitted:u32}
#[derive(Clone,Copy)]struct Dispatch{receive_pass_end:Option<u64>,registry_entered:Option<u64>,selected:Option<u64>,turns:Turns}
#[derive(Clone,Copy)]struct Blocker{transaction:u16,published:Option<u64>,done:Option<u64>}
#[derive(Clone,Copy)]struct Elf{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,dispatch:Dispatch,waits_by_reason:[u32;6],blocker:Option<Blocker>}
struct Registry{elf:Option<Elf>} struct Bounds{first_visible:Option<u64>}struct Close{bounds:Option<Bounds>}
#[derive(Clone,Copy)]struct ModemOwner{write:Option<Write>,elf:Option<Elf>,close:Option<u64>}
impl ModemOwner{fn original_tftp_write_trace(&self)->Option<Write>{self.write}fn service_registry_trace(&self)->Registry{Registry{elf:self.elf}}fn init_close_fifo(&self)->Close{Close{bounds:Some(Bounds{first_visible:self.close})}}fn tftp_wire_counts(&self)->[u32;4]{[1,0,0,1]}}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;8]{[0,0x7fffffff,0,0,0,0,0,0]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,wlan_query:Query}impl RuntimeDomain{CLOCK}
#[path=PANEL_PATH]mod actual;
fn main(){
 let mut pixels=vec![0x00122b52u32;2*1080*2340];let real=unsafe{physical::Display::new(pixels.as_mut_ptr())};let d=Display{real,values:RefCell::new([u32::MAX;30])};
 let mut r=RuntimeDomain{runtime_started:100,hz:1_000_000,error:0,wlan_query:Query};
 let o=ModemOwner{write:Some(Write{final_block:Some(1),peer:Address{node:0,port:0x1f},data:Some(Data{block:1,received_at:Some(200),stages:Stages{final_prepared:true,final_queued:true,dispatch_at:Some(300),prepared_at:Some(400),returned_at:Some(500),queued_at:Some(600)}}),ack:Ack{phase:4,published_at:Some(700),done_at:Some(800)}}),close:Some(900),elf:Some(Elf{received:Some(1000),queued:Some(1400),published:Some(1500),done:Some(1600),dispatch:Dispatch{receive_pass_end:Some(1100),registry_entered:Some(1200),selected:Some(1300),turns:Turns{runs:[0x4120c,0x41101,0,0],omitted:2}},waits_by_reason:[0,12,0,0,0,0],blocker:Some(Blocker{transaction:4,published:Some(1350),done:Some(1375)})})};
 d.replace_line(0,"NATIVE MPSS GLINK QRTR");r.show_tftp_completion(&d,&o);d.replace_line(29,"RESTART WHEN DONE");
 {let v=d.values.borrow();assert_eq!(v[3],1);for row in 4..=18{assert_eq!(v[row],100*(row as u32-3),"row{row}");}assert_eq!(v[19],0x4120c);assert_eq!(v[20],0x41101);assert_eq!(v[21],2);assert_eq!(v[22],12);assert_eq!(v[23],4);assert_eq!(v[24],1250);assert_eq!(v[25],1275);assert_eq!(v[26],4);}
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_STAGE_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 let mut absent=o;absent.write=None;absent.elf=None;r.show_tftp_completion(&d,&absent);{let v=d.values.borrow();assert_eq!(v[3],0);for row in 4..=26{assert_eq!(v[row],u32::MAX,"missing row{row}");}}
 for peer in [Address{node:99,port:0x1f},Address{node:0,port:0x22}]{let mut other=o;other.write.as_mut().unwrap().peer=peer;r.show_tftp_completion(&d,&other);assert_eq!(d.values.borrow()[11],u32::MAX);}
 for bad in 0..3{let mut unproven=o;let w=unproven.write.as_mut().unwrap();match bad{0=>w.data.as_mut().unwrap().stages.final_prepared=false,1=>w.data.as_mut().unwrap().stages.final_queued=false,_=>w.final_block=Some(2)};r.show_tftp_completion(&d,&unproven);for row in [9,10,26]{assert_eq!(d.values.borrow()[row],u32::MAX,"unproven ACK row{row}");}}
 let mut partial=o;partial.write.as_mut().unwrap().data.as_mut().unwrap().stages.queued_at=None;partial.elf.as_mut().unwrap().blocker=None;r.show_tftp_completion(&d,&partial);assert_eq!(d.values.borrow()[8],u32::MAX);for row in 23..=25{assert_eq!(d.values.borrow()[row],u32::MAX);}
 r.hz=0;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[4],u32::MAX);assert_eq!(d.values.borrow()[19],0x4120c);
 r.hz=1_000_000;r.runtime_started=10000;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[4],u32::MAX);
 println!("Stage panel: actual renderer, all boundaries, partial and missing identity, unrelated close, clocks and both framebuffers passed.");
}
'''.replace('DISPLAY_PATH',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',source[a:end]).replace('PANEL_PATH',json.dumps(str(ROOT/'rust/src/pixel_mpss_ack_elf_stages.rs')))
  with tempfile.TemporaryDirectory() as temp:
   p=Path(temp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True,env=os.environ)
if __name__=='__main__':unittest.main()
