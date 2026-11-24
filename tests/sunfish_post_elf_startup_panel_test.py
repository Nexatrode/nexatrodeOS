"""Render actual startup panel: separate ACK identities, absence and both buffers."""
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

class PostElfStartupPanel(unittest.TestCase):
    def test_actual_renderer(self):
        runtime = (ROOT/'rust/src/pixel_mpss_runtime.rs').read_text()
        packed = function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(), 'packed_counts')
        code = r'''
use std::cell::RefCell;
#[path=DISPLAY]mod physical;
struct Display{real:physical::Display,values:RefCell<[u32;30]>,rows:RefCell<Vec<(usize,usize,Vec<(u32,usize)>)>>}
impl Display{
 fn replace_line(&self,n:usize,s:&str){assert!(n<30);assert!(s.len()<=24);self.real.replace_line(n,s);}
 fn hex(&self,n:usize,s:&str,v:u32){assert!(s.len()<=15);self.values.borrow_mut()[n]=v;self.real.hex(n,s,v);}
 fn wire_row(&self,n:usize,lane:usize,v:&[(u32,usize)]){assert!(n<29&&lane<4);assert!(v.iter().all(|(_,w)|*w==8));assert!(v.len()<=4);self.rows.borrow_mut().push((n,lane,v.to_vec()));self.real.wire_row(n,lane,v);}
}
mod diagnostic{pub(super) PACKED}
#[derive(Clone,Copy)]struct Address{port:u32}
#[derive(Clone,Copy)]struct Elf{received:Option<u64>,queued:Option<u64>,published:Option<u64>,done:Option<u64>}
#[derive(Clone,Copy)]struct Reply{phase:u8,published_at:Option<u64>,done_at:Option<u64>}
#[derive(Clone,Copy)]struct Data{received_at:Option<u64>}
#[derive(Clone,Copy)]struct Timing{first_close_at:Option<u64>}
#[derive(Clone,Copy)]struct Write{peer:Address,received_at:Option<u64>,oack:Reply,data:Option<Data>,ack:Reply,final_block:Option<u16>,timing:Timing}
#[derive(Clone,Copy)]struct Ack{destination:Address,flags:u32}
#[derive(Clone,Copy)]struct Post{opcode:u16,path_kind:&'static str,source_port:u32,destination_port:u32,body_bytes:u32}
#[derive(Clone,Copy)]struct Header{meta:u32,source_port:u32,destination_port:u32}
#[derive(Clone,Copy)]struct Exchange{first_tftp_request:Option<Post>,qmi_matches_reference:bool,completed:bool,following:[Option<Header>;4]}
#[derive(Clone,Copy)]struct Trace{elf:Option<Elf>,elf_exchange:Option<Exchange>}
struct Reads{total:u32,kinds:u32,first:[Option<(u32,u32,Option<u64>)>;5]}
struct ModemOwner{trace:Trace,first:Option<Write>,ack:Option<Ack>,reads:Reads}
impl ModemOwner{
 fn service_registry_trace(&self)->Trace{self.trace}
 fn original_tftp_write_trace(&self)->Option<Write>{self.first}
 fn tftp_final_ack_trace(&self)->Option<Ack>{self.ack}
 fn read_sequence(&self)->&Reads{&self.reads}
 fn tftp_wire_counts(&self)->[u32;4]{[7,9,0,1]}
}
struct Server{writes:usize,mcfg:(u32,Option<usize>,u32)}
impl Server{fn completed_count(&self)->usize{self.writes}fn mcfg_progress(&self)->(u32,Option<usize>,u32){self.mcfg}}
struct Query;
impl Query{fn snapshot(&self,_:&ModemOwner)->[u32;10]{[4,0x7fffffff,0,0,1,0,0x31,0,0,0]}}
struct RuntimeDomain{runtime_started:u64,hz:u64,error:u32,ipa_complete_seen:bool,server_check:Server,wlan_query:Query}
impl RuntimeDomain{CLOCK}
#[path=PANEL]mod panel;
fn main(){
 let mut pixels=vec![0u32;1080*2340*2];
 let d=Display{real:unsafe{physical::Display::new(pixels.as_mut_ptr())},values:RefCell::new([0;30]),rows:RefCell::new(vec![])};
 let reply=Reply{phase:4,published_at:Some(140),done_at:Some(150)};
 let mut r=RuntimeDomain{runtime_started:100,hz:1000000,error:0,ipa_complete_seen:true,server_check:Server{writes:2,mcfg:(1,None,0)},wlan_query:Query};
 let post=Post{opcode:1,path_kind:"MCFG TMP",source_port:0x49,destination_port:0x5000,body_bytes:73};
 let header=Header{meta:0x01010080,source_port:0x49,destination_port:0x5000};
 let mut o=ModemOwner{trace:Trace{elf:Some(Elf{received:Some(200),queued:Some(210),published:Some(220),done:Some(230)}),elf_exchange:Some(Exchange{first_tftp_request:Some(post),qmi_matches_reference:true,completed:true,following:[Some(header);4]})},first:Some(Write{peer:Address{port:0x1f},received_at:Some(110),oack:reply,data:Some(Data{received_at:Some(130)}),ack:reply,final_block:Some(1),timing:Timing{first_close_at:Some(160)}}),ack:Some(Ack{destination:Address{port:0x52},flags:31}),reads:Reads{total:9,kinds:0x3e,first:[Some((1,0x45,Some(240))),Some((2,0x46,Some(241))),Some((3,0x47,Some(242))),Some((4,0x48,Some(243))),Some((5,0x49,Some(244)))]}};
 r.show_tftp_completion(&d,&o);
 let v=*d.values.borrow();assert_eq!(v[5],0x1f);assert_eq!(v[9],0x40001);assert_eq!(v[10],40);assert_eq!(v[11],50);assert_eq!(v[14],2);assert_eq!(v[18],0x9003e);assert_eq!(v[25],0x7fffffff);
 let rows=d.rows.borrow();assert_eq!(rows[0].2,vec![(100,8),(110,8),(120,8),(130,8)]);assert_eq!(rows[1].2,vec![(2,8),(0x52,8),(31,8)]);assert_eq!(rows[2].2,vec![(1,8),(0x49,8),(0x5000,8),(73,8)]);assert_eq!(rows[3].2,vec![(1,8),(u32::MAX,8),(0,8)]);assert_eq!(rows[8].2,vec![(140,8),(0x45,8),(1,8)]);assert_eq!(rows[12].2,vec![(144,8),(0x49,8),(5,8)]);assert_eq!(rows.len(),13);drop(rows);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_POST_ELF_PANEL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 for(kind,code)in[("SERVER CHECK",1),("MCFG TMP",2),("OTA FIREWALL RULESET",3),("MODEM EFS",4),("WLANMDSP MBN",5),("OTHER PATH",6),("PATH DECODE ERROR",7)]{o.trace.elf_exchange.as_mut().unwrap().first_tftp_request=Some(Post{path_kind:kind,..post});r.show_tftp_completion(&d,&o);assert_eq!(d.values.borrow()[14],code);}
 r.hz=0;d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);assert!(d.rows.borrow()[0].2.iter().all(|(v,_)|*v==u32::MAX));assert_eq!(d.values.borrow()[5],0x1f);assert_eq!(d.values.borrow()[4],1);
 r.hz=1000000;o.trace=Trace{elf:None,elf_exchange:None};o.first=None;o.ack=None;o.reads=Reads{total:0,kinds:0,first:[None;5]};r.server_check.mcfg=(0,Some(0),0);d.rows.borrow_mut().clear();r.show_tftp_completion(&d,&o);
 for n in [4,5,6,7,8,9,10,11,12,14,16]{assert_eq!(d.values.borrow()[n],u32::MAX);}
 assert_eq!(d.rows.borrow()[1].2,vec![(2,8),(u32::MAX,8),(0,8)]);assert_eq!(d.rows.borrow()[3].2,vec![(0,8),(0,8),(0,8)]);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Original/later ACK identity, all path codes, ELF clocks, absent versus empty MCFG, missing/clockless observations and dual framebuffers passed");
}
'''.replace('#[path=DISPLAY]', '#[path='+json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))+']').replace('PACKED', packed).replace('CLOCK', function(runtime, 'time_from_us')).replace('#[path=PANEL]', '#[path='+json.dumps(str(ROOT/'rust/src/pixel_mpss_post_elf_startup.rs'))+']')
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp); (p/'panel.rs').write_text(code)
            subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-registry-tx3-wire-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
            subprocess.run([str(p/'panel')],check=True)

if __name__ == '__main__': unittest.main()
