"""Render the actual completion panel and time conversion with fixture metadata."""
from pathlib import Path
import json
import os
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

class CompletionPanel(unittest.TestCase):
    def test_actual_panel_missing_identity_clock_and_endpoint_semantics(self):
        source = (ROOT/'rust/src/pixel_mpss_runtime.rs').read_text()
        start = source.index('fn time_from_us(')
        a = source.index('{', start)
        depth, end = 1, a+1
        while depth:
            depth += (source[end] == '{') - (source[end] == '}')
            end += 1
        clock = source[start:end]
        harness = r'''
use std::cell::RefCell;
#[path=DISPLAY_PATH] mod physical;
struct Display{real:physical::Display,values:RefCell<[u32;30]>}
impl Display{
 fn replace_line(&self,n:usize,s:&str){assert!(n<30);self.real.replace_line(n,s);}
 fn hex(&self,n:usize,s:&str,v:u32){assert!(n<30);self.values.borrow_mut()[n]=v;self.real.hex(n,s,v);}
}
#[derive(Clone,Copy)]struct Address{node:u32,port:u32}
#[derive(Clone,Copy)]struct First{peer:Address,transfer_port:u32,oack_at:Option<u64>,first_data_at:Option<u64>,first_close_at:Option<u64>}
#[derive(Clone,Copy)]struct Data{result:Option<u32>,block:u16,body_bytes:u32}
#[derive(Clone,Copy)]struct Ack{phase:u8,published_at:Option<u64>,done_at:Option<u64>}
#[derive(Clone,Copy)]struct Write{peer:Address,data:Option<Data>,ack:Ack,final_block:Option<u16>}
#[derive(Clone,Copy)]struct Elf{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>}
#[derive(Clone,Copy)]struct Bounds{first_visible:Option<u64>}
struct Close{bounds:Option<Bounds>}
struct Registry{elf:Option<Elf>}
mod qcom_glink_fifo{pub enum ServerCheckTail{Consumed(Option<u64>),Absent}}
#[derive(Clone,Copy)]struct ModemOwner{first:Option<First>,write:Option<Write>,elf:Option<Elf>,close:Option<u64>}
impl ModemOwner{
 fn server_check_oacks(&self)->[Option<First>;2]{[self.first,None]}
 fn original_tftp_write_trace(&self)->Option<Write>{self.write}
 fn service_registry_trace(&self)->Registry{Registry{elf:self.elf}}
 fn init_close_fifo(&self)->Close{Close{bounds:Some(Bounds{first_visible:self.close})}}
 fn first_oack_tail(&self)->qcom_glink_fifo::ServerCheckTail{qcom_glink_fifo::ServerCheckTail::Consumed(Some(210))}
 fn wlan_discovery_times(&self)->(Option<u64>,Option<u64>){(None,None)}
 fn tftp_wire_counts(&self)->[u32;4]{[1,0,0,1]}
}
struct Query;impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;8]{[0,0x7fffffff,0,0,0,0,0,0]}}
struct Server;impl Server{fn completed_count(&self)->usize{2}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,wlan_query:Query,server_check:Server}
impl RuntimeDomain{CLOCK_BODY}
#[path=PANEL_PATH]mod actual;
fn main(){
 let mut pixels=vec![0x00122b52u32;2*1080*2340];let real=unsafe{physical::Display::new(pixels.as_mut_ptr())};let d=Display{real,values:RefCell::new([u32::MAX;30])};
 let mut r=RuntimeDomain{runtime_started:100,hz:1_000_000,error:0,wlan_query:Query,server_check:Server};
 let o=ModemOwner{first:Some(First{peer:Address{node:0,port:0x1f},transfer_port:0x5001,oack_at:Some(200),first_data_at:Some(300),first_close_at:Some(750)}),
 write:Some(Write{peer:Address{node:0,port:0x1f},data:Some(Data{result:Some(0),block:1,body_bytes:8}),ack:Ack{phase:4,published_at:Some(500),done_at:Some(600)},final_block:Some(1)}),
 elf:Some(Elf{received:Some(800),queued:Some(850),published:Some(900),done:Some(1000)}),close:Some(700)};
 d.replace_line(0,"NATIVE MPSS GLINK QRTR");r.show_tftp_completion(&d,&o);d.replace_line(29,"RESTART WHEN DONE");
 {let v=d.values.borrow();assert_eq!(v[5],1);assert_eq!(v[8],200);assert_eq!(v[9],0);assert_eq!(v[10],1);assert_eq!(v[11],8);assert_eq!(v[12],1);assert_eq!(v[13],4);assert_eq!(v[14],400);assert_eq!(v[15],500);assert_eq!(v[16],600);assert_eq!(v[17],650);assert_eq!(v[20],800);assert_eq!(v[23],1);assert_eq!(v[27],u32::MAX);}
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(path)=std::env::var("SUNFISH_COMPLETION_PANEL_PPM") {let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(path,b).unwrap();}
 let mut missing=o;missing.write=None;r.show_tftp_completion(&d,&missing);{let v=d.values.borrow();assert_eq!(v[5],0);for row in [7,9,10,11,12,13,14,15,16,23]{assert_eq!(v[row],u32::MAX,"row{row}");}assert_eq!(v[22],2,"other local completions cannot invent original ACK");}
 for address in [Address{node:99,port:0x1f},Address{node:0,port:0x22}] {let mut other=o;other.first.as_mut().unwrap().peer=address;other.write.as_mut().unwrap().peer=address;r.show_tftp_completion(&d,&other);assert_eq!(d.values.borrow()[16],u32::MAX);assert_eq!(d.values.borrow()[23],u32::MAX);}
 let mut equal=o;equal.close=Some(600);r.show_tftp_completion(&d,&equal);assert_eq!(d.values.borrow()[23],0);
 let mut late=o;late.close=Some(590);r.show_tftp_completion(&d,&late);assert_eq!(d.values.borrow()[23],0);
 let mut unknown=o;unknown.close=None;r.show_tftp_completion(&d,&unknown);assert_eq!(d.values.borrow()[23],u32::MAX);
 r.hz=0;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[8],u32::MAX);
 r.hz=1_000_000;r.runtime_started=10000;r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[8],u32::MAX);
 println!("Actual panel: identity/missing endpoints/full-node close matching/clock checks passed; both framebuffers equal.");
}
'''.replace('DISPLAY_PATH', json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('PANEL_PATH', json.dumps(str(ROOT/'rust/src/pixel_mpss_tftp_completion.rs'))).replace('CLOCK_BODY', clock)
        with tempfile.TemporaryDirectory() as temp:
            p = Path(temp)
            (p/'panel.rs').write_text(harness)
            subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
            subprocess.run([str(p/'panel')],check=True,env=os.environ)

if __name__ == '__main__':
    unittest.main()
