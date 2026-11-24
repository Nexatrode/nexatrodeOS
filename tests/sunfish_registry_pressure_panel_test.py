"""Render the bounded registry observer with the actual framebuffer and clock code."""
from pathlib import Path
import json,os,subprocess,tempfile,unittest
ROOT=Path(__file__).resolve().parents[1]
class PressurePanel(unittest.TestCase):
 def test_pressure_panel(self):
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
#[derive(Clone,Copy)]struct ModemOwner{handoff:Handoff,window:Window,write:Option<Write>,elf:Option<Elf>,txn4:Option<Txn4>,close:Option<u64>}
impl ModemOwner{fn registry_handoff_trace(&self)->Handoff{self.handoff}fn registry_window_trace(&self)->Window{self.window}fn original_tftp_write_trace(&self)->Option<Write>{self.write}fn service_registry_trace(&self)->Registry{Registry{elf:self.elf,txn4:self.txn4}}fn init_close_fifo(&self)->Close{Close{bounds:Some(Bounds{first_visible:self.close})}}fn tftp_wire_counts(&self)->[u32;4]{[1,0,0,1]}}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;8]{[0,0x7fffffff,0,0,0,0,0,0]}}
struct Router{turn:bool}impl Router{fn original_final_ack_turn(&self)->bool{self.turn}}
struct RuntimeDomain{server_check:Server,ipa_complete_seen:bool,router:Router,runtime_started:u64,hz:u64,error:u32,wlan_query:Query}impl RuntimeDomain{CLOCK}
#[path=PANEL_PATH]mod actual;
#[derive(Clone,Copy)]struct Pair{older_transaction:u16,newer_transaction:u16,peer:Address,queued_at:Option<u64>,newer_published_at:Option<u64>,older_done_at:Option<u64>,published_before_done:Option<bool>}
#[derive(Clone,Copy)]struct Window{admissions:u32,target:Option<Pair>,predecessor:Option<Prior>}
struct Server;impl Server{fn completed_count(&self)->usize{1}}
#[derive(Clone,Copy)]struct Prior{queued_at:Option<u64>,published_at:Option<u64>,done_at:Option<u64>,publication:Option<PublicationCounters>,publication_frozen:bool}
#[derive(Clone,Copy)]struct Attempt{writer_at:Option<u64>,writer_end:Option<u64>,ready_at:Option<u64>,retry_at:Option<u64>,retry_end:Option<u64>,result:u8}
#[derive(Clone,Copy)]struct Handoff{attempts:u32,target:Option<Attempt>}
#[derive(Clone,Copy)]struct PublicationCounters{bytes:u16,calls:u32,mask:u32,turn_gates:[u32;8],ack:AckCounters}
#[derive(Clone,Copy)]struct AckCounters{sent:u32,blocked:u32,done_ahead:u32,ads_ahead:u32,max_age:Option<u64>}
fn main(){
 let mut pixels=vec![0x00122b52u32;2*1080*2340];let real=unsafe{physical::Display::new(pixels.as_mut_ptr())};let d=Display{real,values:RefCell::new([u32::MAX;30])};
 let mut r=RuntimeDomain{server_check:Server,ipa_complete_seen:true,router:Router{turn:false},runtime_started:100,hz:1_000_000,error:0,wlan_query:Query};
 let counters=PublicationCounters{bytes:88,calls:20,mask:0x34,turn_gates:[1,2,3,4,5,6,7,8],ack:AckCounters{sent:1,blocked:2,done_ahead:3,ads_ahead:4,max_age:Some(1_250_000)}};
 let mut o=ModemOwner{handoff:Handoff{attempts:0,target:None},window:Window{admissions:2,predecessor:Some(Prior{queued_at:Some(200),published_at:Some(300),done_at:Some(700),publication:Some(counters),publication_frozen:true}),target:None},txn4:Some(Txn4{received:Some(400),queued:Some(500),published:Some(600),done:None,last_pressure:None}),write:Some(Write{final_block:Some(1),peer:Address{node:0,port:0x1f},data:Some(Data{block:1,received_at:None,stages:Stages{final_prepared:true,final_queued:true,dispatch_at:None,prepared_at:None,returned_at:None,queued_at:None}}),ack:Ack{phase:4,published_at:None,done_at:None}}),close:None,elf:Some(Elf{received:Some(550),queued:Some(750),published:Some(800),done:Some(900),dispatch:Dispatch{receive_pass_end:None,registry_entered:None,selected:None,turns:Turns{runs:[0;4],omitted:0}},waits_by_reason:[0;6],blocker:None})};
 d.replace_line(0,"NATIVE MPSS GLINK QRTR");r.show_tftp_completion(&d,&o);d.replace_line(29,"RESTART WHEN DONE");
 let expect=[0,2,100,200,600,88,20,0x34,0x10002,0x30004,0x50006,0x70008,1,2,0x30004,1250,300,400,500,450,650,700,800,4,1,0,0];assert_eq!(&d.values.borrow()[2..29],&expect);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_PRESSURE_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 let original=o;o.window.predecessor.as_mut().unwrap().publication_frozen=false;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[3],1);
 o.window.predecessor=None;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[3],0);for row in 4..=17{assert_eq!(d.values.borrow()[row],u32::MAX,"absent row{row}");}
 o=original;o.window.predecessor.as_mut().unwrap().publication.as_mut().unwrap().turn_gates[0]=u32::MAX;o.window.predecessor.as_mut().unwrap().publication.as_mut().unwrap().turn_gates[1]=70000;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[10],u32::MAX);assert_eq!(d.values.borrow()[3],2);
 o=original;o.window.predecessor.as_mut().unwrap().publication.as_mut().unwrap().ack.max_age=None;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[17],u32::MAX);
 for bad in 0..3{o=original;let w=o.write.as_mut().unwrap();match bad{0=>w.data.as_mut().unwrap().stages.final_prepared=false,1=>w.data.as_mut().unwrap().stages.final_queued=false,_=>w.final_block=Some(2)};r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[25],u32::MAX);}
 r.hz=0;r.show_tftp_completion(&d,&original);for row in [4,5,6,17,18,19,20,21,22,23,24]{assert_eq!(d.values.borrow()[row],u32::MAX);}assert_eq!(d.values.borrow()[3],2);assert_eq!(d.values.borrow()[8],20);
 println!("Actual predecessor pressure renderer: all27fields, live/frozen/absent, packed counters/saturation, duration units, missing clocks, ACK identity and dual framebuffers passed.");
}
'''
  code=code.replace('DISPLAY_PATH',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',source[a:end]).replace('PANEL_PATH',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_pressure.rs')))
  with tempfile.TemporaryDirectory() as temp:
   p=Path(temp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','-Adead_code',*(['--cfg','feature=\"sunfish-registry-between-grants\"'] if os.environ.get('SUNFISH_REGISTRY_GRANT_PANEL') else []),str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True,env=os.environ)
if __name__=='__main__':unittest.main()
