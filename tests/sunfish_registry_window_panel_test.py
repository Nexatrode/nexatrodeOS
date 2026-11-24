"""Render the bounded registry observer with the actual framebuffer and clock code."""
from pathlib import Path
import json,os,subprocess,tempfile,unittest
ROOT=Path(__file__).resolve().parents[1]
class WindowPanel(unittest.TestCase):
 def test_window_panel(self):
  source=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text();a=source.index('fn time_from_us(');b=source.index('{',a);depth=1;end=b+1
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
#[derive(Clone,Copy)]struct Publication{transaction:u16}
#[derive(Clone,Copy)]struct Pressure{reason:u8,peer_kind:u8,peer_flags:u8,peer_publication:Option<Publication>}
#[derive(Clone,Copy)]struct Txn4{last_pressure:Option<Pressure>,received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>}
struct Registry{elf:Option<Elf>,txn4:Option<Txn4>} struct Bounds{first_visible:Option<u64>}struct Close{bounds:Option<Bounds>}
#[derive(Clone,Copy)]struct ModemOwner{window:Window,write:Option<Write>,elf:Option<Elf>,txn4:Option<Txn4>,close:Option<u64>}
impl ModemOwner{fn registry_window_trace(&self)->Window{self.window}fn original_tftp_write_trace(&self)->Option<Write>{self.write}fn service_registry_trace(&self)->Registry{Registry{elf:self.elf,txn4:self.txn4}}fn init_close_fifo(&self)->Close{Close{bounds:Some(Bounds{first_visible:self.close})}}fn tftp_wire_counts(&self)->[u32;4]{[1,0,0,1]}}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;8]{[0,0x7fffffff,0,0,0,0,0,0]}}
struct Router{turn:bool}impl Router{fn original_final_ack_turn(&self)->bool{self.turn}}
struct RuntimeDomain{server_check:Server,ipa_complete_seen:bool,router:Router,runtime_started:u64,hz:u64,error:u32,wlan_query:Query}impl RuntimeDomain{CLOCK}
#[path=PANEL_PATH]mod actual;
#[derive(Clone,Copy)]struct Pair{older_transaction:u16,newer_transaction:u16,peer:Address,queued_at:Option<u64>,newer_published_at:Option<u64>,older_done_at:Option<u64>,published_before_done:Option<bool>}
#[derive(Clone,Copy)]struct Window{admissions:u32,target:Option<Pair>}
struct Server;impl Server{fn completed_count(&self)->usize{1}}
fn main(){
 let mut pixels=vec![0x00122b52u32;2*1080*2340];let real=unsafe{physical::Display::new(pixels.as_mut_ptr())};let d=Display{real,values:RefCell::new([u32::MAX;30])};
 let mut r=RuntimeDomain{server_check:Server,ipa_complete_seen:true,router:Router{turn:true},runtime_started:100,hz:1_000_000,error:0,wlan_query:Query};
 let mut o=ModemOwner{window:Window{admissions:2,target:Some(Pair{older_transaction:3,newer_transaction:4,peer:Address{node:0,port:9},queued_at:Some(200),newer_published_at:Some(300),older_done_at:Some(400),published_before_done:Some(true)})},txn4:Some(Txn4{received:Some(500),queued:Some(600),published:Some(700),done:Some(800),last_pressure:None}),write:Some(Write{final_block:Some(1),peer:Address{node:0,port:0x1f},data:Some(Data{block:1,received_at:None,stages:Stages{final_prepared:true,final_queued:true,dispatch_at:None,prepared_at:None,returned_at:None,queued_at:None}}),ack:Ack{phase:4,published_at:Some(1300),done_at:Some(1400)}}),close:Some(1500),elf:Some(Elf{received:Some(900),queued:Some(1000),published:Some(1100),done:Some(1200),dispatch:Dispatch{receive_pass_end:None,registry_entered:None,selected:None,turns:Turns{runs:[0;4],omitted:0}},waits_by_reason:[0;6],blocker:None})};
 d.replace_line(0,"NATIVE MPSS GLINK QRTR");r.show_tftp_completion(&d,&o);d.replace_line(29,"RESTART WHEN DONE");
 let expect=[0,2,3,4,0,9,100,200,300,1,400,500,600,700,800,900,1000,1100,1200,1300,1400,1,4,1,0,0,1];
 assert_eq!(&d.values.borrow()[2..29],&expect);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_WINDOW_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 let original=o;
 o.window.target=None;o.txn4=None;o.elf=None;o.write=None;r.show_tftp_completion(&d,&o);
 for row in 4..=22{assert_eq!(d.values.borrow()[row],u32::MAX,"absent row{row}");}assert_eq!(d.values.borrow()[24],u32::MAX);
 for bad in 0..3 {o=original;let w=o.write.as_mut().unwrap();match bad{0=>w.data.as_mut().unwrap().stages.final_prepared=false,1=>w.data.as_mut().unwrap().stages.final_queued=false,_=>w.final_block=Some(2)};r.show_tftp_completion(&d,&o);for row in [20,21,24]{assert_eq!(d.values.borrow()[row],u32::MAX);}}
 for peer in [Address{node:9,port:0x1f},Address{node:0,port:9}]{o=original;o.write.as_mut().unwrap().peer=peer;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[22],u32::MAX);}
 o=original;o.window.target.as_mut().unwrap().published_before_done=Some(false);r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[11],0);
 o.window.target.as_mut().unwrap().published_before_done=None;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[11],u32::MAX);
 r.hz=0;r.show_tftp_completion(&d,&original);assert_eq!(d.values.borrow()[8],u32::MAX);assert_eq!(d.values.borrow()[11],1);
 r.hz=1_000_000;r.runtime_started=10000;r.show_tftp_completion(&d,&original);assert_eq!(d.values.borrow()[8],u32::MAX);
 println!("Actual registry window renderer: all27 fields, missing/false proof, ACK identity, exact close, absent/reversed clocks and dual framebuffers passed.");
}
'''
  code=code.replace('DISPLAY_PATH',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',source[a:end]).replace('PANEL_PATH',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_window.rs')))
  with tempfile.TemporaryDirectory() as temp:
   p=Path(temp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True,env=os.environ)
if __name__=='__main__':unittest.main()
