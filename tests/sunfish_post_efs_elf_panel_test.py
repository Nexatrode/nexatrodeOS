"""Exercise the actual renderer on both physical buffers with distinct retained events."""
from pathlib import Path
import json,os,subprocess,tempfile,unittest
ROOT=Path(__file__).resolve().parents[1]
def function(source,name):
 a=source.index('fn '+name+'(');end=source.index('{',a)+1;depth=1
 while depth:
  depth+=(source[end]=='{')-(source[end]=='}');end+=1
 return source[a:end]
class Panel(unittest.TestCase):
 def test_actual_renderer(self):
  code=r'''
use std::cell::RefCell;
#[path=DISPLAY]mod physical;
struct Display{real:physical::Display,rows:RefCell<Vec<(usize,Vec<u32>)>>}
impl Display{
 fn replace_line(&self,n:usize,s:&str){assert!(n<30&&s.len()<=24);self.real.replace_line(n,s);}
 fn header_label(&self,n:usize,s:&str){assert!(n<29&&s.len()<=57);self.real.header_label(n,s);}
 fn header_row(&self,n:usize,b:bool,v:&[(u32,usize)]){assert!(b&&n<29&&v.len()<=6);assert!(v.iter().map(|(_,w)|w+1).sum::<usize>()<=57);self.rows.borrow_mut().push((n,v.iter().map(|p|p.0).collect()));self.real.header_row(n,b,v);}
}
mod diagnostic{pub(super) PACKED}
#[derive(Clone,Copy)]struct Address{port:u32}
#[derive(Clone,Copy)]struct Elf{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>}
#[derive(Clone,Copy)]struct Credit{requested:bool,remote_intent:Option<u32>,completed:bool}
#[derive(Clone,Copy)]struct Request{path_kind:&'static str,opcode:u16,source_port:u32,body_bytes:u32}
#[derive(Clone,Copy)]struct Exchange{qmi_matches_reference:bool,host_credit:Option<Credit>,first_tftp_request:Option<Request>}
#[derive(Clone,Copy,Default)]struct Trace{elf:Option<Elf>,elf_exchange:Option<Exchange>,queued_names:u32,completed_names:u32}
#[derive(Clone,Copy)]struct Read{published:Option<u64>,done:Option<u64>,close:Option<u64>,flags:u32}
#[derive(Clone,Copy,Default)]struct Reads{reads:[Option<Read>;4]}
#[derive(Clone,Copy,Default)]struct Pair{flags:u32,admitted:u32,offers:u32}
#[derive(Clone,Copy)]struct State{modem_ready:bool,qmi_uc_ready:bool,indication_requested:bool,indication_sent:bool,initial_boot:bool}
#[derive(Clone,Copy)]struct Ipa{protocol:Option<State>,protocol_complete:bool,failed:bool,pending_record:bool,record_queued:bool,held_response:bool,pending_message_id:Option<u16>}
#[derive(Clone,Copy,Default)]struct Turn{entry:Option<u64>,end:Option<u64>,flags:u32,published:u32}
#[derive(Clone,Copy)]struct Ack{sent:u32,blocked:u32,done_ahead:u32,ads_ahead:u32,max_age:Option<u64>}
#[derive(Clone,Copy)]struct Publication{calls:u32,mask:u32,transaction:u16,bytes:u16,turn_gates:[u32;8],grant_gates:[u32;12],ack:Ack}
#[derive(Clone,Copy)]struct Record{start:Option<u64>,end:Option<u64>,command:u16,parameter:u32,kind:u8,transaction:u16,flags:u8,result:u8}
#[derive(Clone,Copy)]struct Writer{first:Option<u64>,last:Option<u64>,passes:u32,omitted:u32,total:Option<u64>,max_gap:Option<u64>,timed:u32,finished:bool,active:bool,regressed:bool,records:[Option<Record>;8],publication:Option<Publication>}
struct Window{writer:Writer}
struct Mcfg{peer:Address,flags:u32,file_before:Option<u32>,server_result:Option<u32>,last_client:Option<u32>,close_at:Option<u64>}
struct Capture{mcfg:Option<Mcfg>}
struct Cleanup{stage:u32}
#[derive(Clone,Copy)]struct FinalAck{done_at:Option<u64>}
#[derive(Clone,Copy)]struct Write{ack:FinalAck}
struct Inventory{total:u32,kinds:u32}
struct ModemOwner{trace:Trace,reads:Reads,pair:Pair,ipa:Option<Ipa>,turn:Turn,window:Option<Window>,mcfg:Option<Capture>}
impl ModemOwner{
 fn service_registry_trace(&self)->Trace{self.trace}fn elf_window_trace(&self)->Option<&Window>{self.window.as_ref()}fn startup_prerequisites(&self)->Reads{self.reads}fn efs_overlap_trace(&self)->Pair{self.pair}fn pending_mcfg_close(&self)->Option<Address>{None}fn mcfg_close_trace(&self)->Cleanup{Cleanup{stage:6}}fn linux_ipa_report(&self)->Option<Ipa>{self.ipa}fn elf_queued_turn_trace(&self)->Turn{self.turn}fn post_elf_mcfg(&self)->Option<&Capture>{self.mcfg.as_ref()}fn tftp_wire_counts(&self)->[u32;4]{[0,0,0,0]}fn read_sequence(&self)->Inventory{Inventory{total:14,kinds:0x2e}}fn original_tftp_write_trace(&self)->Option<Write>{Some(Write{ack:FinalAck{done_at:Some(115)}})}
}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;10]{[4,0x7fffffff,0,0,0,0,0,0,0,0]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,detail:u32,ipa_complete_seen:bool,wlan_query:Query}
impl RuntimeDomain{CLOCK}
#[path=PANEL]mod panel;
fn main(){
 let mut pixels=vec![0u32;1080*2340*2];let d=Display{real:unsafe{physical::Display::new(pixels.as_mut_ptr())},rows:RefCell::new(vec![])};
 let mut r=RuntimeDomain{runtime_started:100,hz:1000000,error:0,detail:0,ipa_complete_seen:true,wlan_query:Query};
 let read=Read{published:Some(140),done:Some(150),close:Some(160),flags:0xbf};
 let p=Publication{calls:8,mask:0xa0,transaction:5,bytes:88,turn_gates:[1,2,3,4,5,6,7,8],grant_gates:[0,1,2,3,4,5,6,7,8,9,10,11],ack:Ack{sent:1,blocked:2,done_ahead:3,ads_ahead:4,max_age:Some(7)}};
 let mut records=[None;8];for i in 0..8{records[i]=Some(Record{start:Some(200+i as u64*10),end:Some(205+i as u64*10),command:5+i as u16,parameter:0x100+i as u32,kind:4,transaction:5+i as u16,flags:i as u8,result:(i%4)as u8});}
 let w=Writer{first:Some(200),last:Some(275),passes:12,omitted:4,total:Some(40),max_gap:Some(6),timed:8,finished:true,active:false,regressed:false,records,publication:Some(p)};
 let mut o=ModemOwner{trace:Trace{elf:Some(Elf{received:Some(120),queued:Some(190),published:Some(280),done:Some(300)}),elf_exchange:Some(Exchange{qmi_matches_reference:true,host_credit:Some(Credit{requested:true,remote_intent:Some(0),completed:true}),first_tftp_request:Some(Request{path_kind:"MCFG TMP",opcode:1,source_port:0x4a,body_bytes:73})}),queued_names:0x75,completed_names:0x75},reads:Reads{reads:[None,None,Some(read),Some(Read{published:Some(141),..read})]},pair:Pair{flags:0x7f,admitted:1,offers:1},ipa:Some(Ipa{protocol:Some(State{modem_ready:true,qmi_uc_ready:true,indication_requested:true,indication_sent:true,initial_boot:true}),protocol_complete:true,failed:false,pending_record:true,record_queued:false,held_response:true,pending_message_id:Some(0x35)}),turn:Turn{entry:Some(191),end:Some(285),flags:31,published:1},window:Some(Window{writer:w}),mcfg:Some(Capture{mcfg:Some(Mcfg{peer:Address{port:0x4a},flags:0x83,file_before:Some(1),server_result:Some(2),last_client:Some(0),close_at:Some(320)})})};
 let values=|n|d.rows.borrow().iter().find(|(row,_)|*row==n).unwrap().1.clone();
 r.show_post_efs_elf(&d,&o);
 assert_eq!(values(3),[20,90,180,200]);assert_eq!(values(4),[40,50,60,0xbf]);assert_eq!(values(5),[41,50,60,0x1017f]);assert_eq!(values(6),[1,7,0x35f,0x35]);assert_eq!(values(7),[2,1,0x4a,73]);assert_eq!(values(8),[91,185,31,1]);assert_eq!(values(9),[100,175,12,4]);assert_eq!(values(10),[40,6,8,1]);assert_eq!(values(11),[8,0xa0,5,88]);assert_eq!(values(12),[1,2,3,4]);assert_eq!(values(16),[8,9,10,11]);assert_eq!(values(25),[1,2,3,4,7]);assert_eq!(values(26),[0x4a,0x83,1,2,0,220]);assert_eq!(values(27),[1,0x7fffffff,0,0]);assert_eq!(values(28),[15,diagnostic::packed_counts(14,0x2e),0x75,0x75]);
 for i in 0..8{assert_eq!(values(17+i),[100+i as u32*10,105+i as u32*10,((5+i as u32)<<24)|(4<<20)|((i as u32)<<16)|(5+i as u32),0x100+i as u32,(i%4)as u32]);}
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(path)=std::env::var("SUNFISH_POST_EFS_ELF_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(path,b).unwrap();}
 r.hz=0;d.rows.borrow_mut().clear();r.show_post_efs_elf(&d,&o);assert_eq!(values(3),[u32::MAX;4]);assert_eq!(values(10),[u32::MAX,u32::MAX,8,1]);assert_eq!(values(17),[u32::MAX,u32::MAX,0x05400005,0x100,0]);
 r.hz=1000000;o.window.as_mut().unwrap().writer.active=true;o.window.as_mut().unwrap().writer.finished=false;o.window.as_mut().unwrap().writer.regressed=true;o.window.as_mut().unwrap().writer.records[7]=None;d.rows.borrow_mut().clear();r.show_post_efs_elf(&d,&o);assert_eq!(values(10),[40,6,8,6]);assert_eq!(values(24),[u32::MAX;5]);
 o.trace=Trace::default();o.reads=Reads::default();o.ipa=None;o.window=None;o.mcfg=None;d.rows.borrow_mut().clear();r.show_post_efs_elf(&d,&o);assert_eq!(values(3),[u32::MAX;4]);assert_eq!(values(6),[u32::MAX;4]);assert_eq!(values(9),[u32::MAX,u32::MAX,0,0]);assert_eq!(values(11),[0,0,u32::MAX,u32::MAX]);assert_eq!(values(17),[u32::MAX;5]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual post-EFS ELF renderer: distinct records/results, live IPA, EFS identity, missing/clockless, active/regressed and both physical buffers passed");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_post_efs_elf.rs'))).replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us'))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
