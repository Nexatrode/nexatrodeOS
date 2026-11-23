"""Render the actual queue-to-arm diagnostic, including absent clocks and both buffers."""
from pathlib import Path
import json,os,subprocess,tempfile,unittest
ROOT=Path(__file__).resolve().parents[1]
class RegistryPaintPanel(unittest.TestCase):
 def test_actual_renderer(self):
  source=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text();a=source.index('fn time_from_us(');end=source.index('{',a)+1;depth=1
  while depth:depth+=(source[end]=='{')-(source[end]=='}');end+=1
  code=r'''
use std::cell::RefCell;
#[path=GATE_PATH]mod registry_paint;
#[path=DISPLAY_PATH]mod physical;
struct Display{real:physical::Display,values:RefCell<[u32;30]>}
impl Display{fn replace_line(&self,n:usize,s:&str){assert!(n<30);if n==1{assert_eq!(s,"REGISTRY DEFER PAINT");}self.real.replace_line(n,s);}fn hex(&self,n:usize,s:&str,v:u32){assert!(n<30);assert!(s.len()<=15);self.values.borrow_mut()[n]=v;self.real.hex(n,s,v);}}
#[derive(Clone,Copy)]struct Choice{at:Option<u64>,command:u16,parameter:u32,transaction:Option<u16>,result:u8}
#[derive(Clone,Copy)]struct Prearm{queued_flags:u32,first_pass_at:Option<u64>,first_flags:Option<u32>,first_gate:Option<u8>,first_choice:Option<Choice>,passes:u32,own_done_at:Option<u64>}
#[derive(Clone,Copy)]struct Pair{prearm:Prearm,newer_published_at:Option<u64>}
#[derive(Clone,Copy)]struct Window{target:Option<Pair>}
#[derive(Clone,Copy)]struct Phases{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>}
struct Registry{elf:Option<Phases>}
#[derive(Clone,Copy)]struct Io{queued_at:Option<u64>,first_enter_at:Option<u64>,first_end_at:Option<u64>,arm_at:Option<u64>,parts:Option<[u64;4]>}
impl Io{fn delta(&self,_:bool)->Option<[u64;4]>{self.parts}fn total(&self,arm:bool)->Option<u64>{self.parts.map(|p|if arm{p.iter().sum()}else{4})}}
#[derive(Clone,Copy)]struct ModemOwner{window:Window,io:Option<Io>,elf:Option<Phases>}
impl ModemOwner{fn registry_window_trace(&self)->Window{self.window}fn registry_prearm_io(&self)->Option<Io>{self.io}fn service_registry_trace(&self)->Registry{Registry{elf:self.elf}}fn tftp_wire_counts(&self)->[u32;4]{[0;4]}}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;8]{[0;8]}}
struct RuntimeDomain{registry_paint:registry_paint::Gate,runtime_started:u64,hz:u64,error:u32,ipa_complete_seen:bool,wlan_query:Query}
impl RuntimeDomain{CLOCK_SOURCE}
#[path=PANEL_PATH]mod actual;
fn main(){
 let mut pixels=vec![0x00122b52u32;2*1080*2340];let d=Display{real:unsafe{physical::Display::new(pixels.as_mut_ptr())},values:RefCell::new([0;30])};
 let mut r=RuntimeDomain{registry_paint:registry_paint::Gate::default(),runtime_started:100,hz:1_000_000,error:0,ipa_complete_seen:true,wlan_query:Query};
 r.registry_paint.progress(0,true);r.registry_paint.full(0,true,true);r.registry_paint.record(Some(120),Some(130),115,125);
 let mut o=ModemOwner{window:Window{target:Some(Pair{prearm:Prearm{queued_flags:0x1200,first_pass_at:Some(135),first_flags:Some(0x123),first_gate:Some(1),first_choice:Some(Choice{at:Some(137),command:6,parameter:42,transaction:None,result:1}),passes:2,own_done_at:Some(145)},newer_published_at:Some(200)})},io:Some(Io{queued_at:Some(120),first_enter_at:Some(130),first_end_at:Some(140),arm_at:Some(150),parts:Some([1,2,3,4])}),elf:Some(Phases{received:Some(160),queued:Some(170),published:Some(180),done:Some(190)})};
 d.replace_line(0,"NATIVE MPSS GLINK QRTR");r.show_tftp_completion(&d,&o);d.replace_line(29,"RESTART WHEN DONE");
 assert_eq!(&d.values.borrow()[2..29],&[0,20,30,35,40,45,50,0x20001,0x1200,0x123,0xffff0006,42,1,4,10,1,2,3,4,2,70,5,90,100,1,0,0]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_PAINT_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 let original=o;o.window.target.as_mut().unwrap().prearm.first_gate=None;o.window.target.as_mut().unwrap().prearm.passes=u32::MAX;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[9],u32::MAX);
 o=original;o.window.target.as_mut().unwrap().prearm.first_choice.as_mut().unwrap().transaction=Some(4);o.window.target.as_mut().unwrap().prearm.first_choice.as_mut().unwrap().result=4;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[12],0x40006);assert_eq!(d.values.borrow()[14],4);
 o=original;o.window.target=None;o.io=None;o.elf=None;r.show_tftp_completion(&d,&o);for row in (3..=25).filter(|row|*row!=21){assert_eq!(d.values.borrow()[row],u32::MAX,"absent row{row}");}assert_eq!(d.values.borrow()[26],1);
 r.hz=0;r.show_tftp_completion(&d,&original);for row in [3,4,5,6,7,8,15,16,17,18,19,20,22,23,24,25]{assert_eq!(d.values.borrow()[row],u32::MAX);}assert_eq!(d.values.borrow()[14],1);
 println!("All27fields, bounded pass/gate and transaction packing, missing evidence/clock, callback outcome, both framebuffers passed");
}
'''.replace('GATE_PATH',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_paint.rs'))).replace('DISPLAY_PATH',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK_SOURCE',source[a:end]).replace('PANEL_PATH',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_prearm_io.rs')))
  with tempfile.TemporaryDirectory() as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-registry-defer-paint"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
