"""Render frozen TX3 counters and exact receipt-to-registry retry boundaries."""
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
  runtime=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text()
  packed=function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')
  code=r'''

use std::cell::RefCell;
#[path=DISPLAY]mod physical;
struct Display{real:physical::Display,rows:RefCell<Vec<(usize,bool,Vec<(u32,usize)>)>>}
impl Display{
 fn replace_line(&self,n:usize,s:&str){assert!(n<30&&s.len()<=24);self.real.replace_line(n,s);}
 fn hex(&self,n:usize,s:&str,v:u32){self.real.hex(n,s,v);}
 fn header_label(&self,n:usize,s:&str){assert!(n<29&&s.len()<=57);self.real.header_label(n,s);}
 fn header_row(&self,n:usize,second:bool,v:&[(u32,usize)]){assert!(n<29);assert!(v.iter().map(|(_,w)|w+1).sum::<usize>()<=57);self.rows.borrow_mut().push((n,second,v.to_vec()));self.real.header_row(n,second,v);}
}
mod diagnostic{pub(super) PACKED}

#[derive(Clone,Copy,Debug,PartialEq)]struct Address{node:u32,port:u32}
#[derive(Clone,Copy)]struct Elf{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,peer_port:u32,domain_offset:u32,total_domains:Option<u16>,returned_rows:Option<u8>,waits_by_reason:[u32;6],dispatch:Dispatch,blocker:Option<Blocker>}
#[derive(Clone,Copy)]struct Credit{requested:bool,completed:bool,before_reply:Option<[bool;2]>}
#[derive(Clone,Copy)]struct Exchange{qmi_matches_reference:bool,completed:bool,confirm_rx:u32,host_credit:Option<Credit>}
#[derive(Clone,Copy)]struct Dispatch{receive_pass_end:Option<u64>,registry_entered:Option<u64>,selected:Option<u64>,turns:Turns}
#[derive(Clone,Copy)]struct Turns{calls:u32,omitted:u32,runs:[u32;4]}
#[derive(Clone,Copy)]struct Blocker{transaction:u16,flags_at_wait:u8,wait:Option<u64>,published:Option<u64>,done:Option<u64>,yielded_at:Option<u64>,first_turn:Option<u64>}
#[derive(Clone,Copy)]struct Txn4{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>}
#[derive(Clone,Copy,Default)]struct Turn{flags:u32,armed_at:Option<u64>,entry:Option<u64>,end:Option<u64>,received:u32,published:u32,acknowledged:u64}
#[derive(Clone,Copy)]struct Trace{elf:Option<Elf>,elf_exchange:Option<Exchange>,txn4:Option<Txn4>,requests:u32,replies_queued:u32,queued_names:u32,completed_names:u32,name_counts:[u32;7],avs_audio_queries:u32}
#[derive(Clone,Copy)]struct Ack{done_at:Option<u64>}
#[derive(Clone,Copy)]struct Write{ack:Ack}
#[derive(Clone,Copy)]struct Options{present:u32,flags:u32,unknown_count:u32,unknown_key:u32,values:[u32;7]}
#[derive(Clone,Copy)]struct Request{peer:Address,bytes:u32,flags:u32,options:Options,file_before:Option<u32>,server_result:Option<u32>,reply_opcode:u32,reply_number:u32,reply_bytes:u32,reply_options:Options,transfer_port:u32,remote_intent:u32,received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,first_client:Option<u32>,last_client:Option<u32>,first_client_at:Option<u64>,last_client_at:Option<u64>,client_counts:[u32;3],close_at:Option<u64>,close_port:u32,queue_waits:[u32;2]}
#[derive(Clone,Copy)]struct Header{at:Option<u64>,tx:bool,kind:u8,source:u32,destination:u32,tag:u8,a:u32,b:u32}
struct Capture{mcfg:Option<Request>,journal:[Option<Header>;16],total:u32,omitted:u32,bad:u32,other:Option<[u32;4]>}
#[derive(Clone,Copy)]struct Entry{transaction:u16,flags:u8,remote_intent:u32,kind:u8}
#[derive(Clone,Copy)]struct Admission{at:Option<u64>,sequence:u32,reason:u8,count:u8,gates:u16,entries:[Option<Entry>;2]}
#[derive(Clone,Copy)]struct Receipt{at:Option<u64>,sequence:u32,transaction:u16,remote_intent:u32,remaining:u8,matched_last_failure:bool,target:bool}
#[derive(Clone,Copy)]struct PubAck{sent:u32,blocked:u32,done_ahead:u32,ads_ahead:u32,max_age:Option<u64>}
#[derive(Clone,Copy)]struct Publication{transaction:u16,bytes:u16,calls:u32,mask:u32,turn_gates:[u32;8],grant_gates:[u32;12],ack:PubAck}
#[derive(Clone,Copy)]struct WriterRecord{start:Option<u64>,end:Option<u64>,command:u16,parameter:u32,kind:u8,transaction:u16,flags:u8}
#[derive(Clone,Copy,Default)]struct Writer{first:Option<u64>,last:Option<u64>,finished:bool,passes:u32,timed:u32,omitted:u32,active:bool,total:Option<u64>,max_gap:Option<u64>,regressed:bool,publication:Option<Publication>,records:[Option<WriterRecord>;8]}
struct Window{writer:Writer,attempts:u32,first_failure:Option<Admission>,last_failure:Option<Admission>,admitted:Option<Admission>,receipts:[Option<Receipt>;4],receipt_count:u32,receipt_omitted:u32,last_before_admission:Option<u16>,flags:u32,finished:bool}
#[derive(Clone,Copy)]struct Prior{publication:Option<Publication>,publication_frozen:bool,grant:Option<Grant>,queued_at:Option<u64>,published_at:Option<u64>,done_at:Option<u64>}
#[derive(Clone,Copy)]struct OldWindow{predecessor:Option<Prior>,target:Option<PairPublication>}
#[derive(Clone,Copy)]struct RxDone{last_absent:Option<u64>,first_visible:Option<u64>,bytes_ahead:Option<u32>}
#[derive(Clone,Copy)]struct Fifo{peer_tail_at:Option<u64>,completion:Option<RxDone>,finished:bool,history_evictions:u32}
struct ModemOwner{retry:Retry,pool:Option<Pool>,ipa:Option<Ipa>,trace:Trace,first:Option<Write>,cap:Option<Capture>,turn:Turn,window:Option<Window>,old:OldWindow,fifo:Option<Fifo>}
impl ModemOwner{fn elf_completion_turn_taken(&self)->bool{self.window.is_some()}fn elf_window_trace(&self)->Option<&Window>{self.window.as_ref()}fn registry_window_trace(&self)->OldWindow{self.old}fn registry_reply_fifo(&self)->Option<Fifo>{self.fifo}fn elf_queued_turn_trace(&self)->Turn{self.turn}fn service_registry_trace(&self)->Trace{self.trace}fn original_tftp_write_trace(&self)->Option<Write>{self.first}fn post_elf_mcfg(&self)->Option<&Capture>{self.cap.as_ref()}fn tftp_wire_counts(&self)->[u32;4]{[1,2,0,0]}}
struct Server{mcfg:(u32,Option<usize>,u32)}
impl Server{fn mcfg_progress(&self)->(u32,Option<usize>,u32){self.mcfg}}
struct Query;
impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;10]{[4,0x7fffffff,0,0,1,0,0x31,0,0,0]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,detail:u32,ipa_complete_seen:bool,server_check:Server,wlan_query:Query}
impl RuntimeDomain{CLOCK}
#[path=PANEL]mod panel;

#[derive(Clone,Copy,Debug,PartialEq)]struct Slot{kind:u8,flags:u8,op:u16,len:u16,sent:u16,transaction:u16,message:u16,source:Address,destination:Address,remote:u32}
#[derive(Clone,Copy)]struct Full{at:Option<u64>,slots:[Slot;8]}
#[derive(Clone,Copy)]struct Free{at:Option<u64>,slot:u8,cause:u8,empty_mask:u8,original:Slot}
struct Pool{first:Full,latest:Full,failures:u32,first_free:Option<Free>,admitted:Option<u64>,admitted_slot:Option<u8>,admitted_empty:u8,finished:bool}
#[derive(Clone,Copy)]struct State{modem_ready:bool,qmi_uc_ready:bool,indication_requested:bool,indication_sent:bool,initial_boot:bool}
#[derive(Clone,Copy)]struct Ipa{protocol:Option<State>,protocol_complete:bool,failed:bool,pending_record:bool,record_queued:bool,held_response:bool,pending_message_id:Option<u16>}
struct Pair{flags:u32,admitted:u32,offers:u32}struct Cleanup{stage:u32}
impl ModemOwner{fn elf_tx_slots(&self)->Option<&Pool>{self.pool.as_ref()}fn linux_ipa_report(&self)->Option<Ipa>{self.ipa}fn pending_mcfg_close(&self)->Option<Address>{None}fn mcfg_close_trace(&self)->Cleanup{Cleanup{stage:6}}fn efs_overlap_trace(&self)->Pair{Pair{flags:0x7f,admitted:1,offers:1}}}

#[derive(Clone,Copy)]struct Grant{intent:u32,converted_at:Option<u64>,advertised_at:Option<u64>,ack_at:Option<u64>}
#[derive(Clone,Copy)]struct PairPublication{publication:Option<Publication>,publication_frozen:bool}
#[derive(Clone,Copy,Default)]struct Retry{flags:u32,blocker_transaction:Option<u16>,receipt_at:Option<u64>,work_returned_at:Option<u64>,registry_entry_at:Option<u64>,registry_selected_at:Option<u64>,slot_at:Option<u64>,slot_transaction:Option<u16>,slot_sequence:Option<u32>,receipt_counts:[u32;2],return_counts:[u32;2]}
struct Inventory{total:u32,kinds:u32}struct Capacity{flags:u32,result:u8}
impl ModemOwner{fn elf_retry_trace(&self)->Retry{self.retry}fn elf_capacity_turn_trace(&self)->Capacity{Capacity{flags:15,result:1}}fn read_sequence(&self)->Inventory{Inventory{total:14,kinds:0x2e}}}
fn main(){
 let mut pixels=vec![0u32;1080*2340*2];let d=Display{real:unsafe{physical::Display::new(pixels.as_mut_ptr())},rows:RefCell::new(vec![])};
 let mut r=RuntimeDomain{runtime_started:100,hz:1000000,error:0,detail:0,ipa_complete_seen:true,server_check:Server{mcfg:(1,Some(1),0)},wlan_query:Query};
 let turns=Turns{calls:9,omitted:5,runs:[0x11223344,0x55667788,0xaabbccdd,0x12345678]};let dispatch=Dispatch{receive_pass_end:Some(130),registry_entered:Some(135),selected:Some(140),turns};
 let elf=Elf{received:Some(120),queued:Some(900),published:Some(960),done:Some(1000),peer_port:9,domain_offset:0,total_domains:Some(1),returned_rows:Some(1),waits_by_reason:[0,1,7,2,3,4],dispatch,blocker:None};
 let trace=Trace{elf:Some(elf),elf_exchange:None,txn4:Some(Txn4{received:Some(130),queued:Some(150),published:Some(165),done:Some(800)}),requests:5,replies_queued:5,queued_names:0x75,completed_names:0x75,name_counts:[1,0,1,0,1,1,1],avs_audio_queries:1};
 let a=Entry{transaction:3,flags:0x3b,remote_intent:7,kind:4};let b=Entry{transaction:4,flags:0x28,remote_intent:u32::MAX,kind:4};
 let first=Admission{at:Some(160),sequence:1,reason:1,count:2,gates:25,entries:[Some(a),Some(b)]};let last=Admission{at:Some(700),sequence:5,reason:2,gates:1,entries:[Some(a),Some(Entry{flags:0x3b,remote_intent:8,..b})],..first};let admitted=Admission{at:Some(900),sequence:8,reason:0,count:1,gates:0,entries:[Some(a),None]};
 let receipt=Receipt{at:Some(710),sequence:6,transaction:3,remote_intent:7,remaining:1,matched_last_failure:true,target:false};
 let window=Window{writer:Writer{passes:1,..Writer::default()},attempts:7,first_failure:Some(first),last_failure:Some(last),admitted:Some(admitted),receipts:[Some(receipt),Some(Receipt{at:Some(800),sequence:7,transaction:4,remote_intent:8,remaining:0,..receipt}),Some(Receipt{at:Some(1000),sequence:9,transaction:5,remote_intent:0,remaining:0,matched_last_failure:false,target:true}),None],receipt_count:3,receipt_omitted:0,last_before_admission:Some(4),flags:31,finished:true};
 let mut slots=[Slot{kind:1,flags:0x77,op:7,len:52,sent:52,transaction:0xffff,message:0xffff,source:Address{node:1,port:0x5000},destination:Address{node:0,port:0x20},remote:0};8];
 for i in 0..8{slots[i].kind=1+i as u8;slots[i].source.port+=i as u32;slots[i].destination.port+=i as u32;slots[i].remote=i as u32;slots[i].len+=i as u16;slots[i].sent=i as u16;slots[i].transaction=i as u16;slots[i].message=0x20+i as u16;}
 let first_slots=slots;slots[1].remote=0x10000;slots[1].destination.port=0x10049;
 let pool=Pool{first:Full{at:Some(300),slots:first_slots},latest:Full{at:Some(700),slots},failures:5,first_free:Some(Free{at:Some(750),slot:1,cause:1,empty_mask:2,original:first_slots[1]}),admitted:Some(900),admitted_slot:Some(0),admitted_empty:0,finished:true};
 let publication=Publication{transaction:3,bytes:88,calls:17,mask:0xac,turn_gates:[0,1,12,0,0,0,1,1],grant_gates:[0,1,2,3,4,5,6,7,8,9,10,11],ack:PubAck{sent:1,blocked:2,done_ahead:3,ads_ahead:4,max_age:Some(7)}};
 let mut o=ModemOwner{retry:Retry{flags:0x1ff,blocker_transaction:Some(4),receipt_at:Some(800),work_returned_at:Some(850),registry_entry_at:Some(860),registry_selected_at:Some(870),slot_at:Some(710),slot_transaction:Some(3),slot_sequence:Some(6),receipt_counts:[4,2],return_counts:[7,5]},pool:Some(pool),ipa:Some(Ipa{protocol:Some(State{modem_ready:true,qmi_uc_ready:true,indication_requested:true,indication_sent:true,initial_boot:false}),protocol_complete:true,failed:false,pending_record:false,record_queued:false,held_response:false,pending_message_id:None}),trace,first:Some(Write{ack:Ack{done_at:Some(115)}}),cap:None,turn:Turn::default(),window:Some(window),old:OldWindow{target:Some(PairPublication{publication:Some(Publication{transaction:4,bytes:224,calls:2,mask:0xa0,..publication}),publication_frozen:true}),predecessor:Some(Prior{publication:Some(publication),publication_frozen:true,grant:Some(Grant{intent:42,converted_at:Some(130),advertised_at:Some(140),ack_at:Some(145)}),queued_at:Some(135),published_at:Some(140),done_at:Some(710)})},fifo:Some(Fifo{peer_tail_at:Some(145),completion:Some(RxDone{last_absent:Some(680),first_visible:Some(690),bytes_ahead:Some(96)}),finished:true,history_evictions:0})};
 let values=|n|d.rows.borrow().iter().find(|(row,second,_)|*row==n&&*second).unwrap().2.iter().map(|(v,_)|*v).collect::<Vec<_>>();

 r.show_tx3_elf_retry(&d,&o);
 assert_eq!(values(3),[35,40,610,2,3,88]);assert_eq!(values(4),[17,0xac,42,30,40,45]);assert_eq!(values(5),[0,1,12,0]);assert_eq!(values(6),[0,0,1,1]);assert_eq!(values(7),[0,1,2,3]);assert_eq!(values(9),[8,9,10,11]);assert_eq!(values(10),[1,2,3,4,7]);assert_eq!(values(11),[30,50,65,700]);assert_eq!(values(12),[20,800,860,900]);assert_eq!(values(13),[0x1ff,4,610,3,6]);assert_eq!(values(14),[700,750,760,770]);assert_eq!(values(15),[4,2,7,5]);assert_eq!(values(16),[50,10,10,30]);assert_eq!(values(17),[0,1,7,2,3,4]);assert_eq!(values(18),[9,5,0x11223344,0x55667788,0xaabbccdd,0x12345678]);assert_eq!(values(19),[600,0x502,2,1]);assert_eq!(values(20),[0x33b04,7,0x43b04,8]);assert_eq!(values(21),[610,0x30101,7,700,0x40001,8]);assert_eq!(values(22),[900,0x50002,0,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(values(23),[3,0,6,7,9,u32::MAX]);assert_eq!(values(24),[800,8,1,0,4,7]);assert_eq!(values(25),[1,15,1,u32::MAX,u32::MAX,0]);assert_eq!(values(26),[2,2,0xa0,4,224]);assert_eq!(values(27),[15,diagnostic::packed_counts(14,0x2e),0x75,0x75]);assert_eq!(values(28),[1,0x7fffffff,0,0,0x1017f]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_TX3_ELF_RETRY_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 r.hz=0;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(values(14),[u32::MAX;4]);assert_eq!(values(16),[u32::MAX;4]);assert_eq!(values(13),[0x1ff,4,u32::MAX,3,6]);assert_eq!(values(3),[u32::MAX,u32::MAX,u32::MAX,2,3,88]);
 r.hz=1000000;o.retry.work_returned_at=Some(750);d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(values(16)[0],u32::MAX);
 o.retry=Retry{flags:3,blocker_transaction:Some(4),receipt_at:Some(800),..Retry::default()};o.old.predecessor=None;o.old.target=None;o.window=None;o.trace.elf=None;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(values(3),[u32::MAX,u32::MAX,u32::MAX,0,u32::MAX,u32::MAX]);assert_eq!(values(5),[u32::MAX;4]);assert_eq!(values(13),[3,4,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(values(14),[700,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(values(16),[u32::MAX;4]);assert_eq!(values(21),[u32::MAX;6]);assert_eq!(values(26),[0,u32::MAX,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual TX3/retry renderer: frozen predecessor gates, separate TX4 publication, retry boundaries/deltas, exact receipts, missing/clockless/regressed/partial fields and both physical buffers passed");
}
'''.replace('#[path=DISPLAY]','#[path='+json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))+']').replace('PACKED',packed).replace('CLOCK',function(runtime,'time_from_us')).replace('#[path=PANEL]','#[path='+json.dumps(str(ROOT/'rust/src/pixel_mpss_tx3_elf_retry.rs'))+']')
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
