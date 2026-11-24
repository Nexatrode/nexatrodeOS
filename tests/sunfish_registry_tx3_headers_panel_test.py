"""Exercise the actual compact head/tail renderer, detail tags and both buffers."""
from pathlib import Path
import json,os,subprocess,tempfile,unittest
ROOT=Path(__file__).resolve().parents[1]
def function(source,name):
 a=source.index('fn '+name+'(');end=source.index('{',a)+1;depth=1
 while depth:depth+=(source[end]=='{')-(source[end]=='}');end+=1
 return source[a:end]
class RegistryHeadersPanel(unittest.TestCase):
 def test_actual_renderer(self):
  runtime=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text();packed=function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')
  code=r'''
use std::cell::RefCell;
#[path=DISPLAY_PATH]mod physical;
struct Display{real:physical::Display,values:RefCell<[u32;30]>,headers:RefCell<Vec<(usize,usize,Vec<(u32,usize)>)>>}
impl Display{fn replace_line(&self,n:usize,s:&str){assert!(n<30);if n==1{assert_eq!(s,"REGISTRY TX3 WIRE LOG");assert!(s.len()<=24);}self.real.replace_line(n,s);}fn hex(&self,n:usize,s:&str,v:u32){assert!(s.len()<=15);self.values.borrow_mut()[n]=v;self.real.hex(n,s,v);}fn wire_row(&self,row:usize,lane:usize,fields:&[(u32,usize)]){assert!((8..=27).contains(&row));assert!(fields.iter().all(|(_,w)|*w>0&&*w<=8));assert!(fields.iter().map(|(_,w)|w+1).sum::<usize>()<=96);self.headers.borrow_mut().push((row,lane,fields.to_vec()));self.real.wire_row(row,lane,fields);}}
mod diagnostic{pub(super) PACKED}
#[derive(Clone,Copy)]struct Prior{published_at:Option<u64>,done_at:Option<u64>}
#[derive(Clone,Copy)]struct Window{predecessor:Option<Prior>}
#[derive(Clone,Copy)]struct Bounds{first_visible:Option<u64>}
#[derive(Clone,Copy)]struct Fifo{peer_tail_at:Option<u64>,completion:Option<Bounds>}
#[derive(Clone,Copy)]struct Header{at:Option<u64>,tx:bool,command:u16,channel:u16,parameter:u32,bytes:u16,qrtr:Option<[u32;3]>,qmi:Option<(u8,u16,u16)>,del_client:Option<[u32;2]>}
#[derive(Clone,Copy)]struct Wire{headers:[Option<Header>;79],completion:Option<Header>,omitted:u32,unparsed:u32}
#[derive(Clone,Copy)]struct ModemOwner{window:Window,fifo:Option<Fifo>,wire:Option<Wire>}
impl ModemOwner{fn registry_window_trace(&self)->Window{self.window}fn registry_reply_fifo(&self)->Option<Fifo>{self.fifo}fn registry_wire_headers(&self)->Option<Wire>{self.wire}fn tftp_wire_counts(&self)->[u32;4]{[0,0,2,0]}}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;8]{[0,0,0,0,0,0,0,1]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,wlan_query:Query}
impl RuntimeDomain{CLOCK}
#[path=PANEL_PATH]mod actual;
fn main(){let mut pixels=vec![0x00122b52u32;2*1080*2340];let d=Display{real:unsafe{physical::Display::new(pixels.as_mut_ptr())},values:RefCell::new([0;30]),headers:RefCell::new(vec![])};
 let mut r=RuntimeDomain{runtime_started:100,hz:1_000_000,error:0,wlan_query:Query};let first=Header{at:Some(120),tx:true,command:9,channel:1,parameter:7,bytes:104,qrtr:Some([1,0x4003,9]),qmi:Some((2,3,0x21)),del_client:None};let close=Header{at:Some(160),tx:false,command:9,channel:2,parameter:11,bytes:68,qrtr:Some([6,u32::MAX,u32::MAX]),qmi:None,del_client:Some([0,0x49])};let done=Header{at:Some(180),tx:false,command:14,channel:2,parameter:7,bytes:8,qrtr:None,qmi:None,del_client:None};let mut headers=[None;79];headers[0]=Some(first);headers[78]=Some(close);
 let mut o=ModemOwner{window:Window{predecessor:Some(Prior{published_at:Some(120),done_at:Some(180)})},fifo:Some(Fifo{peer_tail_at:Some(125),completion:Some(Bounds{first_visible:Some(175)})}),wire:Some(Wire{headers,completion:Some(done),omitted:6,unparsed:1})};
 d.replace_line(0,"NATIVE MPSS GLINK QRTR");r.show_tftp_completion(&d,&o);d.replace_line(29,"RESTART WHEN DONE");assert_eq!(&d.values.borrow()[2..8],&[0,0x60001,20,25,75,80]);assert_eq!(d.values.borrow()[28],0x20001);
 let h=d.headers.borrow();assert_eq!(h.len(),3);assert_eq!(h[0],(8,0,vec![(20,8),(1,1),(9,4),(1,4),(7,8),(104,4),(1,2),(0x4003,8),(9,8),(2,1),(3,8),(0x21,8)]));assert_eq!((h[1].0,h[1].1),(27,2));assert_eq!(&h[1].2[6..],&[(6,2),(u32::MAX,8),(u32::MAX,8),(0xD,1),(0,8),(0x49,8)]);assert_eq!((h[2].0,h[2].1),(27,3));assert_eq!(h[2].2[9],(0xF,1));drop(h);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 d.headers.borrow_mut().clear();let saved=o.wire;let mut full=saved.unwrap();for (i,h) in full.headers.iter_mut().enumerate(){*h=Some(Header{at:Some(120+i as u64),..first});}o.wire=Some(full);r.show_tftp_completion(&d,&o);assert_eq!(d.headers.borrow().len(),80);for(i,h)in d.headers.borrow().iter().enumerate(){assert_eq!((h.0,h.1),(8+i/4,i%4));}assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);o.wire=saved;
 if let Ok(p)=std::env::var("SUNFISH_TX3_HEADERS_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 d.headers.borrow_mut().clear();r.hz=0;r.show_tftp_completion(&d,&o);assert_eq!(d.headers.borrow()[0].2[0],(u32::MAX,8));assert_eq!(d.values.borrow()[4],u32::MAX);assert_eq!(d.values.borrow()[3],0x60001);
 o.wire=None;o.window.predecessor=None;o.fifo=None;d.headers.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert!(d.headers.borrow().is_empty());for row in 3..=7{assert_eq!(d.values.borrow()[row],u32::MAX);}
 println!("Actual compact80record layout, QMI/DEL_CLIENT/unknown tags, omitted/unparsed packing, clockless/missing window and both buffers passed");}
'''.replace('DISPLAY_PATH',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('PACKED',packed).replace('CLOCK',function(runtime,'time_from_us')).replace('PANEL_PATH',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_tx3_headers.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code);subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-registry-tx3-wire-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True);subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
