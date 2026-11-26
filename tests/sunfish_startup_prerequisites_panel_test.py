"""Exercise the actual renderer on both physical framebuffers, including absent clocks."""
from pathlib import Path
import json,os,subprocess,tempfile,unittest
from sunfish_post_elf_startup_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
class Panel(unittest.TestCase):
 def test_actual_dual_framebuffer_renderer(self):
  render=function((ROOT/'rust/src/pixel_mpss_post_elf_outcome.rs').read_text(),'show_startup_prerequisites')
  clock=function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')
  packed=function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')
  code=r'''
use std::cell::RefCell;
#[path=DISPLAY]mod physical;
struct Display{real:physical::Display,rows:RefCell<Vec<(usize,bool,Vec<(u32,usize)>)>>}
impl Display{
 fn replace_line(&self,n:usize,s:&str){assert!(n<30&&s.len()<=24);if n==1{assert_eq!(s,if cfg!(feature="sunfish-tftp-arrival-order"){"EARLY TFTP ARRIVAL ORDER"}else{"EARLY STARTUP REPLIES"});}self.real.replace_line(n,s);}
 fn header_label(&self,n:usize,s:&str){assert!(n<29&&s.len()<=57);self.real.header_label(n,s);}
 fn header_row(&self,n:usize,second:bool,v:&[(u32,usize)]){assert!(n<29);assert!(v.iter().map(|(_,w)|w).sum::<usize>()+v.len()-1<=57);self.rows.borrow_mut().push((n,second,v.to_vec()));self.real.header_row(n,second,v);}
}
mod diagnostic{pub(super) PACKED}
#[derive(Clone,Copy)]struct Address{port:u32}
#[derive(Clone,Copy)]struct Phase{phase:u8,published_at:Option<u64>,done_at:Option<u64>}
#[derive(Clone,Copy)]struct Data{received_at:Option<u64>}
#[derive(Clone,Copy)]struct Timing{first_close_at:Option<u64>}
#[derive(Clone,Copy)]struct Write{peer:Address,received_at:Option<u64>,oack:Phase,data:Option<Data>,ack:Phase,timing:Timing}
#[derive(Clone,Copy)]struct Elf{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>}
#[derive(Clone,Copy)]struct Credit{requested:bool,remote_intent:Option<u32>,completed:bool}
#[derive(Clone,Copy)]struct Exchange{qmi_matches_reference:bool,host_credit:Option<Credit>}
#[derive(Clone,Copy)]struct Trace{elf:Option<Elf>,elf_exchange:Option<Exchange>,queued_names:u32,completed_names:u32}
#[derive(Clone,Copy)]struct Read{kind:u32,peer:Address,flags:u32,seq:Option<u64>,request_bytes:u32,rsize:u32,options_present:u32,options_flags:u32,result:u32,client:u32,received:Option<u64>,dispatched:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,close:Option<u64>,opcode:u32,number:u32,reply_bytes:u32,local_port:u32,attempts:u32,waits:[u32;2]}
impl Read{fn record_sequence(&self)->Option<u64>{self.seq}}
struct Prereqs{reads:[Option<Read>;4],total:[u32;2],omitted:[u32;2]}
struct Reads{total:u32,kinds:u32}
struct Close{stage:u32}
struct ModemOwner{first:Option<Write>,trace:Trace,p:Prereqs}
impl ModemOwner{
 fn original_tftp_write_trace(&self)->Option<Write>{self.first}
 fn service_registry_trace(&self)->Trace{self.trace}
 fn mcfg_close_trace(&self)->Close{Close{stage:6}}
 fn pending_mcfg_close(&self)->Option<Address>{None}
 fn startup_prerequisites(&self)->&Prereqs{&self.p}
 fn tftp_wire_counts(&self)->[u32;4]{[1,9,0,1]}
 fn read_sequence(&self)->Reads{Reads{total:9,kinds:0x2e}}
}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;10]{[4,0x7fffffff,0,0,0,0,0,0,0,0]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,detail:u32,ipa_complete_seen:bool,wlan_query:Query}
impl RuntimeDomain{CLOCK RENDER}
fn main(){
 let mut pixels=vec![0u32;1080*2340*2];let d=Display{real:unsafe{physical::Display::new(pixels.as_mut_ptr())},rows:RefCell::new(vec![])};
 let phase=Phase{phase:4,published_at:Some(140),done_at:Some(150)};
 let read=Read{kind:1,peer:Address{port:0x21},flags:255,seq:Some(0x10000001e),request_bytes:85,rsize:0,options_present:16,options_flags:0,result:0,client:0x50009,received:Some(200),dispatched:Some(210),queued:Some(220),published:Some(230),done:Some(240),close:Some(250),opcode:5,number:1,reply_bytes:19,local_port:0x5004,attempts:2,waits:[1,0]};
 let mut o=ModemOwner{first:Some(Write{peer:Address{port:0x1f},received_at:Some(110),oack:phase,data:Some(Data{received_at:Some(130)}),ack:phase,timing:Timing{first_close_at:Some(160)}}),trace:Trace{elf:Some(Elf{received:Some(300),queued:Some(310),published:Some(320),done:Some(330)}),elf_exchange:Some(Exchange{qmi_matches_reference:true,host_credit:Some(Credit{requested:true,remote_intent:Some(1),completed:true})}),queued_names:0x3e,completed_names:0x3e},p:Prereqs{reads:[Some(read),None,Some(Read{kind:2,..read}),None],total:[3,2],omitted:[1,0]}};
 let mut r=RuntimeDomain{runtime_started:100,hz:1000000,error:0,detail:0,ipa_complete_seen:true,wlan_query:Query};
 r.show_startup_prerequisites(&d,&o);let values=|n|d.rows.borrow().iter().find(|(row,_,_)|*row==n).unwrap().2.iter().map(|(v,_)|*v).collect::<Vec<_>>();
 assert_eq!(values(3),[0x1f,10,40,30]);assert_eq!(values(4),[4,40,50,60]);assert_eq!(values(5),[200,210,220,230]);assert_eq!(values(6),[1,7,0x3e,0x3e]);assert_eq!(values(8),[1,0x21,255,30,1]);assert_eq!(values(9),[85,0,16,0,0,0x50009]);assert_eq!(values(10),[100,110,120]);assert_eq!(values(11),[130,140,150]);assert_eq!(values(12),[5,1,19,0x5004,2,1,0]);assert_eq!(values(13),[u32::MAX,u32::MAX,0,u32::MAX,u32::MAX]);assert_eq!(values(18)[0],2);assert_eq!(values(28),[0x9002e,3,2,1,0]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_STARTUP_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 r.hz=0;d.rows.borrow_mut().clear();r.show_startup_prerequisites(&d,&o);assert_eq!(values(10),[u32::MAX;3]);assert_eq!(values(11),[u32::MAX;3]);assert_eq!(values(9),[85,0,16,0,0,0x50009]);
 r.hz=1000000;o.first=None;o.trace.elf=None;o.trace.elf_exchange=None;o.p.reads=[None;4];d.rows.borrow_mut().clear();r.show_startup_prerequisites(&d,&o);assert_eq!(values(4),[u32::MAX;4]);assert_eq!(values(5),[u32::MAX;4]);assert_eq!(values(8),[u32::MAX,u32::MAX,0,u32::MAX,u32::MAX]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);println!("Actual startup renderer: identities, zero versus absent options, paired clocks, omissions and both buffers passed");
}
'''.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('PACKED',packed).replace('CLOCK',clock).replace('RENDER',render)
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code);subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024',*(['--cfg','feature="sunfish-tftp-arrival-order"'] if os.environ.get('SUNFISH_ARRIVAL_PANEL') else []),'--cfg','feature="sunfish-registry-tx3-wire-headers"','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True);subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
