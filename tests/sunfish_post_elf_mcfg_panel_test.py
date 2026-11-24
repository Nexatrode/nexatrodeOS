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

class PostElfMcfgPanel(unittest.TestCase):
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
#[derive(Clone,Copy)]struct Elf{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>}
#[derive(Clone,Copy)]struct Credit{requested:bool,completed:bool,before_reply:Option<[bool;2]>}
#[derive(Clone,Copy)]struct Exchange{qmi_matches_reference:bool,completed:bool,confirm_rx:u32,host_credit:Option<Credit>}
#[derive(Clone,Copy)]struct Trace{elf:Option<Elf>,elf_exchange:Option<Exchange>}
#[derive(Clone,Copy)]struct Ack{done_at:Option<u64>}
#[derive(Clone,Copy)]struct Write{ack:Ack}
#[derive(Clone,Copy)]struct Options{present:u32,flags:u32,unknown_count:u32,unknown_key:u32,values:[u32;7]}
#[derive(Clone,Copy)]struct Request{peer:Address,bytes:u32,flags:u32,options:Options,file_before:Option<u32>,server_result:Option<u32>,reply_opcode:u32,reply_number:u32,reply_bytes:u32,reply_options:Options,transfer_port:u32,remote_intent:u32,received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>,first_client:Option<u32>,last_client:Option<u32>,first_client_at:Option<u64>,last_client_at:Option<u64>,client_counts:[u32;3],close_at:Option<u64>,close_port:u32,queue_waits:[u32;2]}
#[derive(Clone,Copy)]struct Header{at:Option<u64>,tx:bool,kind:u8,source:u32,destination:u32,tag:u8,a:u32,b:u32}
struct Capture{mcfg:Option<Request>,journal:[Option<Header>;16],total:u32,omitted:u32,bad:u32,other:Option<[u32;4]>}
struct ModemOwner{trace:Trace,first:Option<Write>,cap:Option<Capture>}
impl ModemOwner{fn service_registry_trace(&self)->Trace{self.trace}fn original_tftp_write_trace(&self)->Option<Write>{self.first}fn post_elf_mcfg(&self)->Option<&Capture>{self.cap.as_ref()}fn tftp_wire_counts(&self)->[u32;4]{[1,2,0,0]}}
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
 let options=Options{present:31,flags:0,unknown_count:0,unknown_key:u32::MAX,values:[7680,1000,10,0,0,u32::MAX,u32::MAX]};
 let reply_options=Options{present:15,flags:0,values:[512,1000,1,1,u32::MAX,u32::MAX,u32::MAX],..options};
 let request=Request{peer:Address{port:0x4a},bytes:73,flags:255,options,file_before:Some(1),server_result:Some(0),reply_opcode:6,reply_number:u32::MAX,reply_bytes:45,reply_options,transfer_port:0x5005,remote_intent:0x31,received:Some(240),queued:Some(250),published:Some(260),done:Some(270),first_client:Some(0x50009),last_client:Some(0x50009),first_client_at:Some(280),last_client_at:Some(280),client_counts:[0,1,2],close_at:Some(290),close_port:0x4a,queue_waits:[3,4]};
 let header=Header{at:Some(300),tx:false,kind:6,source:0xfffffffe,destination:0xfffffffe,tag:13,a:0,b:0x4a};
 let mut o=ModemOwner{trace:Trace{elf:Some(Elf{received:Some(200),queued:Some(210),published:Some(220),done:Some(230)}),elf_exchange:Some(Exchange{qmi_matches_reference:true,completed:true,confirm_rx:1,host_credit:Some(Credit{requested:true,completed:true,before_reply:Some([true,true])})})},first:Some(Write{ack:Ack{done_at:Some(150)}}),cap:Some(Capture{mcfg:Some(request),journal:core::array::from_fn(|i|Some(Header{at:Some(300+i as u64),..header})),total:40,omitted:24,bad:1,other:Some([8,0x55,93,1])})};
 let values=|n|d.rows.borrow().iter().find(|(row,second,_)|*row==n&&*second).unwrap().2.iter().map(|(v,_)|*v).collect::<Vec<_>>();
 r.show_tftp_completion(&d,&o);
 assert_eq!(values(3),[100,110,120,130]);assert_eq!(values(4),[1,1,15,1]);assert_eq!(values(5),[0x4a,73,255,1]);assert_eq!(values(7),[7680,1000,10,0]);assert_eq!(values(8),[0,u32::MAX,u32::MAX]);assert_eq!(values(9),[0,6,u32::MAX,45]);assert_eq!(values(13),[140,150,160,170]);assert_eq!(values(14),[0x50009,0x50009,1,2]);assert_eq!(values(16),[40,24,1,0x30004]);assert_eq!(values(25),[50,130]);assert_eq!(values(26),[1,1,0,1]);
 let rows=d.rows.borrow();let j=rows.iter().filter(|(n,_,_)|(17..=24).contains(n)).collect::<Vec<_>>();assert_eq!(j.len(),16);for(i,h)in j.iter().enumerate(){assert_eq!(h.2[0].0,200+i as u32);}drop(rows);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_POST_MCFG_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 // Rejection and absent-file state must not be presented as a successful OACK.
 {let m=o.cap.as_mut().unwrap().mcfg.as_mut().unwrap();m.flags=3;m.file_before=None;m.server_result=Some(17);m.reply_opcode=u32::MAX;m.reply_bytes=0;}
 d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert_eq!(values(5)[3],u32::MAX);assert_eq!(values(9),[17,u32::MAX,u32::MAX,0]);
 r.hz=0;d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert_eq!(values(13),[u32::MAX;4]);assert_eq!(values(14)[0],0x50009);
 r.hz=1000000;o.cap=None;o.trace=Trace{elf:None,elf_exchange:None};o.first=None;r.server_check.mcfg=(0,Some(0),0);d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert_eq!(values(5),[u32::MAX,u32::MAX,0,u32::MAX]);assert_eq!(values(26),[0,0,0,1]);assert_eq!(values(3),[u32::MAX;4]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual dual-framebuffer renderer: request/reply options including zero, absent/rejected replies, exact client ending, bounded journal, prior ACK/ELF clocks and clockless cases passed");
}
'''.replace('#[path=DISPLAY]', '#[path='+json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))+']').replace('PACKED', packed).replace('CLOCK', function(runtime, 'time_from_us')).replace('#[path=PANEL]', '#[path='+json.dumps(str(ROOT/'rust/src/pixel_mpss_post_elf_mcfg.rs'))+']')
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp); (p/'panel.rs').write_text(code)
            subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-registry-tx3-wire-headers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','--cfg','feature="sunfish-wrq-post-return-callers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
            subprocess.run([str(p/'panel')],check=True)

if __name__ == '__main__': unittest.main()
