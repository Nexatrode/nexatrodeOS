"""Connect actual retained outcome rows without joining unrelated request identities."""
from pathlib import Path
import json, os, re, subprocess, tempfile, unittest
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
BASE='0d89a814'
def without_outcome(path,s):
 from sunfish_elf_digest_outcome_test import without_digest
 s=without_digest(path,s)
 s=re.sub(r'/\* READY_OUTCOME_BEGIN \*/\n.*?/\* READY_OUTCOME_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-ready-startup-outcome = ["sunfish-registry-ready-handoffs"]\n','')
 return s
class Outcome(unittest.TestCase):
 def test_display_only_parent_boundary(self):
  files=subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']
  for f in files:self.assertEqual(without_outcome(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
  s=(ROOT/'rust/src/pixel_mpss_ready_startup_outcome.rs').read_text()
  for v in ['unsafe','with_io','dispatch','queue_reply','rmtfs_event_clock','intent_ack_clock']:self.assertNotIn(v,s)
  s=(ROOT/'rust/src/pixel_mpss_tx3_elf_retry.rs').read_text();self.assertLess(s.index('self.show_ready_startup_outcome('),s.index('if owner.elf_tx_slots().is_some()'))
 def test_physical_outcome_renderer(self):
  code=r'''
use std::cell::RefCell;
#[path=DISPLAY]mod physical;
struct Display{real:physical::Display,rows:RefCell<Vec<(usize,Vec<u32>)>>,title:RefCell<String>}
impl Display{
 fn replace_line(&self,n:usize,s:&str){assert!(n<29&&s.len()<=24);if n==1{*self.title.borrow_mut()=s.into();}self.real.replace_line(n,s);}
 fn header_label(&self,n:usize,s:&str){assert!(n<29&&s.len()<=57);self.real.header_label(n,s);}
 fn header_row(&self,n:usize,second:bool,v:&[(u32,usize)]){assert!(second&&n<29);assert!(v.iter().map(|(_,w)|w+1).sum::<usize>()<=57);assert!(!self.rows.borrow().iter().any(|(i,_)|*i==n));self.rows.borrow_mut().push((n,v.iter().map(|(v,_)|*v).collect()));self.real.header_row(n,second,v);}
}
#[derive(Clone,Copy,Default)]struct Address{port:u32}
#[derive(Clone,Copy,Default)]struct Timed{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,waits_by_reason:[u32;6]}
#[derive(Clone,Copy,Default)]struct Publication{calls:u32,mask:u32}
#[derive(Clone,Copy,Default)]struct Prior{queued_at:Option<u64>,published_at:Option<u64>,done_at:Option<u64>,publication:Option<Publication>,publication_frozen:bool}
#[derive(Clone,Copy,Default)]struct Window{predecessor:Option<Prior>,target:Option<Prior>}
#[derive(Clone,Copy,Default)]struct Credit{requested:bool,remote_intent:Option<u32>,completed:bool}
#[derive(Clone,Copy,Default)]struct Post{path_kind:&'static str,opcode:u16,source_port:u32,body_bytes:u32}
#[derive(Clone,Copy,Default)]struct Exchange{qmi_matches_reference:bool,completed:bool,host_credit:Option<Credit>,first_tftp_request:Option<Post>}
#[derive(Clone,Copy,Default)]struct Trace{elf:Option<Timed>,txn4:Option<Timed>,elf_exchange:Option<Exchange>}
#[derive(Clone,Copy,Default)]struct Handoff{flags:u32,arms:u32,receipts:u32,services:u32,result:u32}
#[derive(Clone,Copy,Default)]struct State{modem_ready:bool,qmi_uc_ready:bool,indication_requested:bool,indication_sent:bool,initial_boot:bool}
#[derive(Clone,Copy,Default)]struct Report{protocol:Option<State>,protocol_complete:bool,failed:bool,pending_record:bool,record_queued:bool,held_response:bool,registration_requests:u32,driver_requests:u32,publications_committed:u32,responses_accepted:u32,pending_message_id:Option<u16>,timeout_causes:u32,ignored_unhandled_requests:u32,last_ignored_unhandled_id:Option<u16>}
#[derive(Clone,Copy,Default)]struct Ack{published_at:Option<u64>,done_at:Option<u64>,phase:u8}
#[derive(Clone,Copy,Default)]struct Data{received_at:Option<u64>}
#[derive(Clone,Copy,Default)]struct Timing{first_close_at:Option<u64>}
#[derive(Clone,Copy,Default)]struct Write{peer:Address,received_at:Option<u64>,oack:Ack,data:Option<Data>,ack:Ack,timing:Timing,final_block:Option<u16>}
#[derive(Clone,Copy,Default)]struct Read{kind:u32,peer:Address,flags:u32,opcode:u32,number:u32,client:u32,received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,close:Option<u64>,result:u32}
#[derive(Clone,Copy,Default)]struct Early{reads:[Option<Read>;4],total:[u32;2],omitted:[u32;2]}
#[derive(Clone,Copy,Default)]struct Request{peer:Address,flags:u32,sequence:Option<u64>,reply_opcode:u32,reply_number:u32,received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,close_at:Option<u64>,canceled_at:Option<u64>,file_before:Option<u32>,server_result:Option<u32>,transfer_port:u32,first_client:Option<u32>,last_client:Option<u32>}
impl Request{fn record_sequence(&self)->Option<u64>{self.sequence}}
#[derive(Clone,Copy,Default)]struct Capture{mcfg:Option<Request>,other:Option<[u32;4]>}
#[derive(Clone,Copy,Default)]struct Close{stage:u32,observed:u32,committed:u32}
#[derive(Clone,Copy,Default)]struct Reads{total:u32,kinds:u32}
#[derive(Default)]struct ModemOwner{trace:Trace,window:Window,handoff:Handoff,report:Option<Report>,original:Option<Write>,early:Early,cap:Option<Capture>,close:Close,reads:Reads,wire:[u32;4]}
impl ModemOwner{
 fn service_registry_trace(&self)->Trace{self.trace}fn registry_window_trace(&self)->Window{self.window}fn ready_handoff_trace(&self)->Handoff{self.handoff}fn pending_mcfg_close(&self)->Option<Address>{None}fn mcfg_close_trace(&self)->Close{self.close}fn linux_ipa_report(&self)->Option<Report>{self.report}fn original_tftp_write_trace(&self)->Option<Write>{self.original}fn startup_prerequisites(&self)->&Early{&self.early}fn post_elf_mcfg(&self)->Option<&Capture>{self.cap.as_ref()}fn read_sequence(&self)->Reads{self.reads}fn tftp_wire_counts(&self)->[u32;4]{self.wire}
}
struct Server;impl Server{fn mcfg_progress(&self)->(u32,Option<usize>,u32){(1,Some(1),0)}}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;10]{[0,0x7fffffff,0,0,0,0,0,0,0,0]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,detail:u32,ipa_complete_seen:bool,server_check:Server,wlan_query:Query}
impl RuntimeDomain{CLOCK}
#[path=PANEL]mod panel;
fn main(){
 let mut pixels=vec![0u32;1080*2340*2];let d=Display{real:unsafe{physical::Display::new(pixels.as_mut_ptr())},rows:RefCell::new(vec![]),title:RefCell::new(String::new())};
 let mut r=RuntimeDomain{runtime_started:100,hz:1000000,error:0,detail:0,ipa_complete_seen:true,server_check:Server,wlan_query:Query};
 let mut o=ModemOwner::default();
 o.window.predecessor=Some(Prior{queued_at:Some(120),published_at:Some(130),done_at:Some(140),publication:Some(Publication{calls:3,mask:0x3fa4}),publication_frozen:true});o.window.target=Some(Prior{publication:Some(Publication{calls:4,mask:0x183fa4}),publication_frozen:true,..Prior::default()});
 o.trace.txn4=Some(Timed{received:Some(200),queued:Some(210),published:Some(220),done:Some(230),..Timed::default()});o.trace.elf=Some(Timed{received:Some(300),queued:Some(310),published:Some(320),done:Some(330),waits_by_reason:[0,1,2,3,4,5]});
 o.trace.elf_exchange=Some(Exchange{qmi_matches_reference:true,completed:true,host_credit:Some(Credit{requested:true,remote_intent:Some(0),completed:false}),first_tftp_request:Some(Post{path_kind:"MCFG TMP",opcode:1,source_port:0x4a,body_bytes:73})});
 o.report=Some(Report{protocol:Some(State{modem_ready:true,qmi_uc_ready:true,indication_requested:true,indication_sent:true,..State::default()}),protocol_complete:false,pending_record:true,record_queued:true,pending_message_id:Some(0x35),registration_requests:1,driver_requests:2,publications_committed:3,responses_accepted:1,ignored_unhandled_requests:2,last_ignored_unhandled_id:Some(0x77),..Report::default()});
 o.original=Some(Write{peer:Address{port:0x1f},received_at:Some(105),oack:Ack{published_at:Some(110),..Ack::default()},data:Some(Data{received_at:Some(115)}),ack:Ack{published_at:Some(119),done_at:Some(121),phase:4},timing:Timing{first_close_at:Some(125)},final_block:Some(1)});
 for i in 0..4{o.early.reads[i]=Some(Read{kind:1+i as u32/2,peer:Address{port:0x21+i as u32},flags:0xbf,opcode:5,number:1,client:0x50001,received:Some(400+i as u64*10),queued:Some(401+i as u64*10),published:Some(402+i as u64*10),done:Some(403+i as u64*10),close:Some(404+i as u64*10),result:i as u32});}
 o.early.total=[3,4];o.early.omitted=[1,2];o.reads=Reads{total:17,kinds:0x2e};o.close=Close{stage:6,observed:2,committed:2};
 o.cap=Some(Capture{mcfg:Some(Request{peer:Address{port:0x4a},flags:0x18f,sequence:Some(0x10000002a),reply_opcode:6,reply_number:u32::MAX,received:Some(500),queued:Some(510),close_at:Some(520),canceled_at:Some(530),file_before:Some(1),server_result:Some(0),transfer_port:0x5009,first_client:Some(0x50002),last_client:Some(0x50003),..Request::default()}),other:Some([8,0x55,93,1])});
 let values=|n|d.rows.borrow().iter().find(|(i,_)|*i==n).unwrap().1.clone();
 r.show_ready_startup_outcome(&d,&o);assert_eq!(*d.title.borrow(),"READY STARTUP OUTCOME");assert_eq!(d.rows.borrow().len(),27);
 assert_eq!(values(3),[20,30,40]);assert_eq!(values(4),[100,110,120,130]);assert_eq!(values(5),[200,210,220,230]);assert_eq!(values(6),[2,3,0x3fa4,2,4,0x183fa4]);assert_eq!(values(7),[0;5]);assert_eq!(values(8),[0,1,2,3,4,5]);
 assert_eq!(values(9),[1,1,1,1,0]);assert_eq!(values(10),[0xcf,1,2,3,1]);assert_eq!(values(11),[0x35,0,0,2,0x77]);assert_eq!(values(12),[0x1f,5,10,15]);assert_eq!(values(13),[19,21,25,4,1]);
 assert_eq!(values(14),[1,0x21,0xbf,5,1,0x50001]);assert_eq!(values(21),[330,331,332,333,334,3]);assert_eq!(values(22),[2,1,0x4a,73]);assert_eq!(values(23),[0x4a,0x18f,42,1,6,u32::MAX]);assert_eq!(values(24),[400,410,u32::MAX,u32::MAX,420,430]);assert_eq!(values(25),[1,0,0x5009,0x50002,0x50003]);assert_eq!(values(26),[17,0x2e,3,4,1,2]);assert_eq!(values(27),[1,1,0,2,2]);assert_eq!(values(28),[1,0x7fffffff,0,0,8,0x55]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_READY_OUTCOME_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 // A later live IPA completion and exact host-credit return remain separate observations.
 {let p=o.report.as_mut().unwrap();p.protocol_complete=true;p.pending_record=false;p.record_queued=false;p.pending_message_id=None;p.publications_committed=4;}o.trace.elf_exchange.as_mut().unwrap().host_credit.as_mut().unwrap().completed=true;o.handoff=Handoff{flags:31,arms:2,receipts:1,services:1,result:1};
 d.rows.borrow_mut().clear();r.show_ready_startup_outcome(&d,&o);assert_eq!(values(7),[31,2,1,1,1]);assert_eq!(values(9),[1;5]);assert_eq!(values(10),[0x1f,1,2,4,1]);assert_eq!(values(11)[0],u32::MAX);
 for(name,kind)in[("SERVER CHECK",1),("MCFG TMP",2),("OTA FIREWALL RULESET",3),("MODEM EFS",4),("WLANMDSP MBN",5),("OTHER PATH",6),("UNKNOWN",7)]{o.trace.elf_exchange.as_mut().unwrap().first_tftp_request.as_mut().unwrap().path_kind=name;d.rows.borrow_mut().clear();r.show_ready_startup_outcome(&d,&o);assert_eq!(values(22)[0],kind);}
 r.hz=0;d.rows.borrow_mut().clear();r.show_ready_startup_outcome(&d,&o);assert_eq!(values(3),[u32::MAX;3]);assert_eq!(&values(21)[..5],&[u32::MAX;5]);assert_eq!(values(23)[2..4],[42,1]);
 r.hz=1000000;o.window.predecessor.as_mut().unwrap().queued_at=Some(90);d.rows.borrow_mut().clear();r.show_ready_startup_outcome(&d,&o);assert_eq!(values(3),[u32::MAX,30,40]);
 o=ModemOwner::default();d.rows.borrow_mut().clear();r.show_ready_startup_outcome(&d,&o);assert_eq!(values(5),[u32::MAX;4]);assert_eq!(values(9),[u32::MAX;5]);assert_eq!(values(10),[u32::MAX;5]);assert_eq!(values(14),[u32::MAX,u32::MAX,0,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(values(24),[u32::MAX;6]);assert_eq!(values(26),[0;6]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual outcome renderer passed: separate live/historical IPA, exact credit, four reads, identity/cancel fields, seven paths, omissions, missing/regressed clocks, all rows, both framebuffer halves");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_ready_startup_outcome.rs')))
  with tempfile.TemporaryDirectory()as t:
   p=Path(t);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
