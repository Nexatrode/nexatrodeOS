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

class ElfQueuedPanel(unittest.TestCase):
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
mod diagnostic{pub(super) PACKED}

#[derive(Clone,Copy)]struct Address{port:u32}
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
struct ModemOwner{trace:Trace,first:Option<Write>,cap:Option<Capture>,turn:Turn}
impl ModemOwner{fn elf_queued_turn_trace(&self)->Turn{self.turn}fn service_registry_trace(&self)->Trace{self.trace}fn original_tftp_write_trace(&self)->Option<Write>{self.first}fn post_elf_mcfg(&self)->Option<&Capture>{self.cap.as_ref()}fn tftp_wire_counts(&self)->[u32;4]{[1,2,0,0]}}
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
 let elf=Elf{received:Some(200),queued:Some(270),published:Some(280),done:Some(290),peer_port:9,domain_offset:0,total_domains:Some(1),returned_rows:Some(1),waits_by_reason:[0,2,0,0,0,0],dispatch:Dispatch{receive_pass_end:Some(210),registry_entered:Some(220),selected:Some(230),turns:Turns{calls:2,omitted:0,runs:[0x00041202,0,0,0]}},blocker:Some(Blocker{transaction:4,flags_at_wait:10,wait:Some(231),published:Some(240),done:Some(250),yielded_at:Some(251),first_turn:Some(232)})};
 let trace=Trace{elf:Some(elf),elf_exchange:Some(Exchange{qmi_matches_reference:true,completed:true,confirm_rx:1,host_credit:Some(Credit{requested:true,completed:true,before_reply:Some([true,true])})}),txn4:Some(Txn4{received:Some(160),queued:Some(170),published:Some(240),done:Some(250)}),requests:5,replies_queued:5,queued_names:0x75,completed_names:0x75,name_counts:[1,0,1,0,1,1,1],avs_audio_queries:1};
 let options=Options{present:15,flags:0,unknown_count:0,unknown_key:u32::MAX,values:[7680,1000,10,0,u32::MAX,u32::MAX,u32::MAX]};
 let request=Request{peer:Address{port:0x4b},bytes:73,flags:0xbf,options,file_before:None,server_result:Some(0),reply_opcode:5,reply_number:1,reply_bytes:19,reply_options:options,transfer_port:0x5007,remote_intent:0,received:Some(300),queued:Some(310),published:Some(320),done:Some(330),first_client:None,last_client:None,first_client_at:None,last_client_at:None,client_counts:[0;3],close_at:Some(340),close_port:0x4b,queue_waits:[0;2]};
 let mut o=ModemOwner{trace,first:Some(Write{ack:Ack{done_at:Some(150)}}),cap:Some(Capture{mcfg:Some(request),journal:[None;16],total:42,omitted:26,bad:0,other:Some([8,0x4f,93,1])}),turn:Turn{flags:31,armed_at:Some(270),entry:Some(271),end:Some(281),received:2,published:3,acknowledged:1}};
 let values=|n|d.rows.borrow().iter().find(|(row,second,_)|*row==n&&*second).unwrap().2.iter().map(|(v,_)|*v).collect::<Vec<_>>();
 r.show_tftp_completion(&d,&o);assert_eq!(values(3),[100,170,180,190]);assert_eq!(values(5),[10,10,10,40]);assert_eq!(values(6),[10,10,80,90]);assert_eq!(values(7),[170,171,181,31]);assert_eq!(values(8),[2,3,1,10]);assert_eq!(values(9),[9,0,1,1]);assert_eq!(values(10),[1,1,15,1]);assert_eq!(values(11),[0,2,0,0]);assert_eq!(values(13),[0x41202,0,0,0]);assert_eq!(values(14),[4,10,131,140]);assert_eq!(values(15),[150,151,132]);assert_eq!(values(16),[60,70,140,150]);assert_eq!(values(20),[50,190]);assert_eq!(values(21),[0x4b,0xbf,1,19]);assert_eq!(values(22),[200,220,230,240]);assert_eq!(values(23),[8,0x4f,93,1]);assert_eq!(values(26),[42,26,0]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_ELF_QUEUED_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 // Attempt without publication must stay distinguishable from later completion.
 o.turn.flags=15;o.turn.end=Some(275);o.trace.elf.as_mut().unwrap().published=None;o.trace.elf.as_mut().unwrap().done=None;d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert_eq!(values(7)[3],15);assert_eq!(values(6),[u32::MAX;4]);
 r.hz=0;d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert_eq!(values(7),[u32::MAX,u32::MAX,u32::MAX,15]);assert_eq!(values(9),[9,0,1,1]);
 r.hz=1000000;o.trace.elf=None;o.trace.elf_exchange=None;o.trace.txn4=None;o.cap=None;o.first=None;o.turn=Turn::default();d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert_eq!(values(3),[u32::MAX;4]);assert_eq!(values(7),[u32::MAX,u32::MAX,u32::MAX,0]);assert_eq!(values(14),[u32::MAX;4]);assert_eq!(values(21),[u32::MAX,0,u32::MAX,0]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual ELF queued-turn renderer: phase partition, attempt versus publication, exact prior blocker/ACK, outcomes, missing/clockless values and both buffers passed");
}
'''.replace('#[path=DISPLAY]', '#[path='+json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))+']').replace('PACKED', packed).replace('CLOCK', function(runtime, 'time_from_us')).replace('#[path=PANEL]', '#[path='+json.dumps(str(ROOT/'rust/src/pixel_mpss_elf_queued_turn.rs'))+']')
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp); (p/'panel.rs').write_text(code)
            subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-registry-tx3-wire-headers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','--cfg','feature="sunfish-wrq-post-return-callers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
            subprocess.run([str(p/'panel')],check=True)

if __name__ == '__main__': unittest.main()
