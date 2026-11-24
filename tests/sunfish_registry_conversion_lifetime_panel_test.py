"""Exercise the actual 27-field conversion-lifetime renderer and both buffers."""
from pathlib import Path
import json, os, subprocess, tempfile, unittest
ROOT=Path(__file__).resolve().parents[1]
class ConversionLifetimePanel(unittest.TestCase):
 def test_actual_renderer(self):
  source=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text();a=source.index('fn time_from_us(');end=source.index('{',a)+1;depth=1
  while depth:depth+=(source[end]=='{')-(source[end]=='}');end+=1
  code=r'''
use std::cell::RefCell;
#[path=DISPLAY_PATH]mod physical;
struct Display{real:physical::Display,values:RefCell<[u32;30]>}
impl Display{fn replace_line(&self,n:usize,s:&str){assert!(n<30);if n==1{assert_eq!(s,"REGISTRY TX4 TURN LIFE");assert!(s.len()<=24);}self.real.replace_line(n,s);}fn hex(&self,n:usize,s:&str,v:u32){assert!(n<30);self.values.borrow_mut()[n]=v;self.real.hex(n,s,v);}}
#[derive(Clone,Copy)]struct Entry{at:Option<u64>,flags:u32,backing:Option<u32>,token_request:Option<u32>,gate:Option<u8>}
#[derive(Clone,Copy)]struct Choice{at:Option<u64>,command:u16,parameter:u32,transaction:Option<u16>,result:u8}
#[derive(Clone,Copy)]struct Turn{intent:u32,request_intent:u32,bytes:u16,sent:u16,armed_at:Option<u64>,entry:Option<Entry>,choice:Option<Choice>,exit:Option<(u8,Option<u64>)>,advertised_at:Option<u64>,ack_at:Option<u64>}
#[derive(Clone,Copy)]struct Lifetime{arms:u32,first:Option<Turn>}
#[derive(Clone,Copy)]struct Pressure{calls:u32,mask:u32}
#[derive(Clone,Copy)]struct Pair{conversion:Lifetime,publication:Option<Pressure>,publication_frozen:bool}
#[derive(Clone,Copy)]struct Window{target:Option<Pair>}
#[derive(Clone,Copy)]struct Tx4{queued:Option<u64>,published:Option<u64>}
struct Registry{txn4:Option<Tx4>}
#[derive(Clone,Copy)]struct ModemOwner{window:Window,tx4:Option<Tx4>}
impl ModemOwner{fn registry_window_trace(&self)->Window{self.window}fn service_registry_trace(&self)->Registry{Registry{txn4:self.tx4}}fn tftp_wire_counts(&self)->[u32;4]{[0;4]}}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;8]{[0;8]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,ipa_complete_seen:bool,wlan_query:Query}
impl RuntimeDomain{CLOCK_SOURCE}
#[path=PANEL_PATH]mod actual;
fn main(){
 let mut pixels=vec![0x00122b52u32;2*1080*2340];let real=unsafe{physical::Display::new(pixels.as_mut_ptr())};let d=Display{real,values:RefCell::new([0;30])};
 let mut r=RuntimeDomain{runtime_started:100,hz:1_000_000,error:0,ipa_complete_seen:true,wlan_query:Query};
 let first=Turn{intent:27,request_intent:42,bytes:88,sent:0,armed_at:Some(150),entry:Some(Entry{at:Some(200),flags:0x1a3f,backing:Some(27),token_request:Some(42),gate:Some(11)}),choice:Some(Choice{at:Some(220),command:9,parameter:18,transaction:Some(4),result:1}),exit:Some((1,Some(250))),advertised_at:Some(300),ack_at:Some(400)};
 let mut o=ModemOwner{window:Window{target:Some(Pair{conversion:Lifetime{arms:2,first:Some(first)},publication:Some(Pressure{calls:20,mask:0x3fa4}),publication_frozen:true})},tx4:Some(Tx4{queued:Some(120),published:Some(280)})};
 d.replace_line(0,"NATIVE MPSS GLINK QRTR");r.show_tftp_completion(&d,&o);d.replace_line(29,"RESTART WHEN DONE");
 assert_eq!(&d.values.borrow()[2..29],&[0,20,180,2,50,27,42,0x580000,100,0x1a3f,11,27,42,120,0x40009,18,1,150,1,200,300,20,0x3fa4,2,1,0,0]);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_LIFETIME_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 let original=o;o.window.target.as_mut().unwrap().publication_frozen=false;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[25],1);
 o=original;let t=o.window.target.as_mut().unwrap().conversion.first.as_mut().unwrap();t.sent=88;t.entry.as_mut().unwrap().gate=None;t.choice.as_mut().unwrap().result=0;t.choice.as_mut().unwrap().transaction=None;t.exit=Some((2,None));r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[9],0x580058);assert_eq!(d.values.borrow()[12],u32::MAX);assert_eq!(d.values.borrow()[16],0xffff0009);assert_eq!(d.values.borrow()[18],0);assert_eq!(d.values.borrow()[19],u32::MAX);assert_eq!(d.values.borrow()[20],2);
 o=original;o.window.target.as_mut().unwrap().conversion.first.as_mut().unwrap().choice.as_mut().unwrap().result=4;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[18],4);
 o=original;o.window.target.as_mut().unwrap().conversion.first=None;r.show_tftp_completion(&d,&o);for row in 6..=22{assert_eq!(d.values.borrow()[row],if row==20{0}else{u32::MAX},"unarmed row{row}");}assert_eq!(d.values.borrow()[5],2);
 o=original;o.window.target=None;o.tx4=None;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[5],0);assert_eq!(d.values.borrow()[25],0);assert_eq!(d.values.borrow()[3],u32::MAX);
 r.hz=0;r.show_tftp_completion(&d,&original);for row in [3,4,6,10,15,19,21,22]{assert_eq!(d.values.borrow()[row],u32::MAX);}assert_eq!(d.values.borrow()[11],0x1a3f);assert_eq!(d.values.borrow()[18],1);
 println!("All27fields, identity labels, packed length/txn, missing gate/clock, live/frozen/unarmed and selected-without-callback outcomes, both framebuffers passed");
}
'''.replace('DISPLAY_PATH',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK_SOURCE',source[a:end]).replace('PANEL_PATH',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_conversion_lifetime.rs')))
  with tempfile.TemporaryDirectory() as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
