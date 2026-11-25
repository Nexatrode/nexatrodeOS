"""Render the actual bounded MCFG panel against identity-rich synthetic snapshots."""
from pathlib import Path
import json, os, subprocess, tempfile, unittest

ROOT = Path(__file__).resolve().parents[1]

def function(source, name):
    a = source.index('fn ' + name + '(')
    end = source.index('{', a) + 1
    depth = 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[a:end]

class ElfTxSlotsPanel(unittest.TestCase):
    def test_actual_renderer(self):
        runtime = (ROOT/'rust/src/pixel_mpss_runtime.rs').read_text()
        packed = function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(), 'packed_counts')
        code = r'''
use std::cell::RefCell;
#[path=DISPLAY]mod physical;
struct Display{real:physical::Display,rows:RefCell<Vec<(usize,bool,Vec<(u32,usize)>)>>}
impl Display{
 fn replace_line(&self,n:usize,s:&str){assert!(n<30&&s.len()<=24);self.real.replace_line(n,s);}
 fn hex(&self,n:usize,s:&str,v:u32){self.real.hex(n,s,v);}
 fn header_label(&self,n:usize,s:&str){assert!(n<29&&s.len()<=32);self.real.header_label(n,s);}
 fn header_row(&self,n:usize,second:bool,v:&[(u32,usize)]){assert!(n<29);assert!(v.iter().map(|(_,w)|w+1).sum::<usize>()<=64);self.rows.borrow_mut().push((n,second,v.to_vec()));self.real.header_row(n,second,v);}
}
mod diagnostic{pub(super) PACKED
pub(super) DELTA}

#[derive(Clone,Copy,PartialEq,Eq)]struct Address{node:u32,port:u32}
#[derive(Clone,Copy)]struct Elf{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,peer_port:u32,domain_offset:u32,total_domains:Option<u16>,returned_rows:Option<u8>,waits_by_reason:[u32;6],dispatch:Dispatch,blocker:Option<Blocker>,io:Io}
#[derive(Clone,Copy)]struct Io{received:Option<[u64;12]>,receive_pass_end:Option<[u64;12]>,queued:Option<[u64;12]>,published:Option<[u64;12]>}
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
struct Window{attempts:u32,first_failure:Option<Admission>,last_failure:Option<Admission>,admitted:Option<Admission>,receipts:[Option<Receipt>;4],receipt_count:u32,receipt_omitted:u32,last_before_admission:Option<u16>,flags:u32,finished:bool}
#[derive(Clone,Copy)]struct Prior{queued_at:Option<u64>,published_at:Option<u64>,done_at:Option<u64>}
#[derive(Clone,Copy)]struct OldWindow{predecessor:Option<Prior>}
#[derive(Clone,Copy)]struct RxDone{last_absent:Option<u64>,first_visible:Option<u64>,bytes_ahead:Option<u32>}
#[derive(Clone,Copy)]struct Fifo{peer_tail_at:Option<u64>,completion:Option<RxDone>,finished:bool,history_evictions:u32}

pub mod qcom_glink_qmi{
 #[derive(Clone,Copy,PartialEq,Eq)]pub struct ElfTxSlot{pub kind:u8,pub flags:u8,pub op:u16,pub len:u16,pub sent:u16,pub transaction:u16,pub message:u16,pub source:super::Address,pub destination:super::Address,pub remote:u32,pub qrtr_type:u32}
}
mod personal_os_rust_server{pub use crate::qcom_glink_qmi;}
#[derive(Clone,Copy)]struct Full{at:Option<u64>,slots:[qcom_glink_qmi::ElfTxSlot;8]}
#[derive(Clone,Copy)]struct Free{at:Option<u64>,slot:u8,cause:u8,empty_mask:u8,original:qcom_glink_qmi::ElfTxSlot}
struct Slots{first:Full,latest:Full,failures:u32,first_free:Option<Free>,admitted:Option<u64>,admitted_slot:Option<u8>,admitted_empty:u8,finished:bool}

struct ModemOwner{slots:Option<Slots>,trace:Trace,first:Option<Write>,cap:Option<Capture>,turn:Turn,window:Option<Window>,old:OldWindow,fifo:Option<Fifo>}
impl ModemOwner{fn elf_tx_slots(&self)->Option<&Slots>{self.slots.as_ref()}fn elf_completion_turn_taken(&self)->bool{self.window.is_some()}fn elf_window_trace(&self)->Option<&Window>{self.window.as_ref()}fn registry_window_trace(&self)->OldWindow{self.old}fn registry_reply_fifo(&self)->Option<Fifo>{self.fifo}fn elf_queued_turn_trace(&self)->Turn{self.turn}fn service_registry_trace(&self)->Trace{self.trace}fn original_tftp_write_trace(&self)->Option<Write>{self.first}fn post_elf_mcfg(&self)->Option<&Capture>{self.cap.as_ref()}fn tftp_wire_counts(&self)->[u32;4]{[1,2,0,0]}}
struct Server{mcfg:(u32,Option<usize>,u32)}
impl Server{fn mcfg_progress(&self)->(u32,Option<usize>,u32){self.mcfg}}
struct Query;
impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;10]{[4,0x7fffffff,0,0,1,0,0x31,0,0,0]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,ipa_complete_seen:bool,server_check:Server,wlan_query:Query}
impl RuntimeDomain{CLOCK}
#[path=PANEL]mod panel;
fn main(){
 let mut pixels=vec![0u32;1080*2340*2];let d=Display{real:unsafe{physical::Display::new(pixels.as_mut_ptr())},rows:RefCell::new(vec![])};
 let mut r=RuntimeDomain{runtime_started:100,hz:1000000,error:0,ipa_complete_seen:true,server_check:Server{mcfg:(1,Some(1),0)},wlan_query:Query};
 let turns=Turns{calls:1,omitted:0,runs:[0x41101,0,0,0]};let dispatch=Dispatch{receive_pass_end:Some(130),registry_entered:Some(135),selected:Some(140),turns};
 let elf=Elf{received:Some(120),queued:Some(190),published:Some(210),done:Some(220),peer_port:9,domain_offset:0,total_domains:Some(1),returned_rows:Some(1),waits_by_reason:[0,2,0,0,0,0],dispatch,blocker:None,io:Io{received:Some([10;12]),receive_pass_end:Some([20;12]),queued:Some([40;12]),published:Some([50;12])}};
 let trace=Trace{elf:Some(elf),elf_exchange:None,txn4:Some(Txn4{received:Some(130),queued:Some(150),published:Some(165),done:Some(200)}),requests:5,replies_queued:5,queued_names:0x75,completed_names:0x75,name_counts:[1,0,1,0,1,1,1],avs_audio_queries:1};
 let a=Entry{transaction:3,flags:0x3b,remote_intent:7,kind:4};let b=Entry{transaction:4,flags:0x3b,remote_intent:8,kind:4};
 let first=Admission{at:Some(160),sequence:1,reason:1,count:2,gates:25,entries:[Some(a),Some(Entry{flags:0x28,remote_intent:u32::MAX,..b})]};
 let last=Admission{at:Some(170),sequence:2,gates:1,entries:[Some(a),Some(b)],..first};let admitted=Admission{at:Some(190),sequence:4,reason:0,count:1,gates:0,entries:[Some(b),None]};
 let receipt=Receipt{at:Some(180),sequence:3,transaction:3,remote_intent:7,remaining:1,matched_last_failure:true,target:false};
 let window=Window{attempts:3,first_failure:Some(first),last_failure:Some(last),admitted:Some(admitted),receipts:[Some(receipt),Some(Receipt{at:Some(200),sequence:5,transaction:4,remote_intent:8,remaining:0,..receipt}),Some(Receipt{at:Some(220),sequence:6,transaction:5,remote_intent:7,remaining:0,matched_last_failure:false,target:true}),None],receipt_count:3,receipt_omitted:0,last_before_admission:Some(3),flags:31,finished:true};
 let opts=Options{present:0x17,flags:0,unknown_count:0,unknown_key:u32::MAX,values:[7680,1000,10,u32::MAX,0,u32::MAX,u32::MAX]};
 let m=Request{peer:Address{node:0,port:0x4a},bytes:73,flags:0x83,options:opts,file_before:Some(1),server_result:Some(2),reply_opcode:u32::MAX,reply_number:u32::MAX,reply_bytes:0,reply_options:opts,transfer_port:u32::MAX,remote_intent:u32::MAX,received:Some(300),queued:None,published:None,done:None,first_client:None,last_client:None,first_client_at:None,last_client_at:None,client_counts:[0;3],close_at:Some(320),close_port:0x4a,queue_waits:[0;2]};
 let rslot=qcom_glink_qmi::ElfTxSlot{kind:6,flags:0x77,op:6,len:88,sent:0,transaction:u16::MAX,message:u16::MAX,source:Address{node:1,port:0x5005},destination:Address{node:0,port:0x49},remote:12,qrtr_type:1};
 let first=Full{at:Some(160),slots:[rslot;8]};let mut latest=Full{at:Some(170),..first};latest.slots[3].sent=88;
 let slots=Slots{first,latest,failures:6,first_free:Some(Free{at:Some(180),slot:3,cause:1,empty_mask:8,original:rslot}),admitted:Some(190),admitted_slot:Some(3),admitted_empty:0,finished:true};
 let mut o=ModemOwner{slots:Some(slots),trace,first:Some(Write{ack:Ack{done_at:Some(115)}}),cap:Some(Capture{mcfg:Some(m),journal:[None;16],total:44,omitted:28,bad:0,other:Some([8,0x4e,93,1])}),turn:Turn{flags:31,armed_at:Some(190),entry:Some(191),end:Some(211),received:1,published:1,acknowledged:0},window:Some(window),old:OldWindow{predecessor:Some(Prior{queued_at:Some(135),published_at:Some(140),done_at:Some(180)})},fifo:Some(Fifo{peer_tail_at:Some(145),completion:Some(RxDone{last_absent:Some(175),first_visible:Some(176),bytes_ahead:Some(96)}),finished:true,history_evictions:3})};
 let values=|n|d.rows.borrow().iter().find(|(row,second,_)|*row==n&&*second).unwrap().2.iter().map(|(v,_)|*v).collect::<Vec<_>>();
 r.show_tftp_completion(&d,&o);assert_eq!(values(3),[20,90,110,120]);assert_eq!(values(4),[60,70,6,1]);assert_eq!(values(5),[80,3,1,8]);assert_eq!(values(6),[0x06770006,0x5005,0x49,12]);assert_eq!(values(7),[88<<16,0xffffffff,1,0]);assert_eq!(values(14),values(6));assert_eq!(values(15),[(88<<16)|88,0xffffffff,1,0]);assert_eq!(values(24),[8,3,0,1]);assert_eq!(values(26),[10,5,5,50]);assert_eq!(values(27),[1,0x7fffffff,0,0]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_ELF_WINDOW_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 r.hz=0;d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert_eq!(values(5),[u32::MAX,3,1,8]);assert_eq!(values(6),[0x06770006,0x5005,0x49,12]);
 r.hz=1000000;o.slots=None;o.window=None;o.trace.elf=None;d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert_eq!(values(4),[u32::MAX,u32::MAX,0,0]);assert_eq!(values(5),[u32::MAX;4]);assert_eq!(values(8),[u32::MAX;4]);assert_eq!(values(23),[u32::MAX;4]);assert_eq!(values(24),[u32::MAX;4]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual ELF TX slot renderer: eight identities, packed lengths and QMI IDs, exact first free, absent/clockless data, dual buffers and width bounds passed");

}
'''.replace('#[path=DISPLAY]', '#[path='+json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))+']').replace('PACKED', packed).replace('DELTA',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'elf_io_delta')).replace('CLOCK', function(runtime, 'time_from_us')).replace('#[path=PANEL]', '#[path='+json.dumps(str(ROOT/'rust/src/pixel_mpss_elf_tx_slots.rs'))+']')
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp); (p/'panel.rs').write_text(code)
            subprocess.run(['rustc','+nightly-2026-09-25',*(['--cfg','feature="sunfish-elf-first-slot-turn"'] if os.getenv('SUNFISH_ELF_SLOT_PANEL') else []),'--edition=2024','--cfg','feature="sunfish-registry-tx3-wire-headers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','--cfg','feature="sunfish-wrq-post-return-callers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
            subprocess.run([str(p/'panel')],check=True)

if __name__ == '__main__': unittest.main()
