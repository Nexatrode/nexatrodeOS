from pathlib import Path
import json,os,subprocess,tempfile,unittest
from sunfish_registry_receipt_fifo_panel_test import receipt_fixture_code
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
class Panel(unittest.TestCase):
 def test_actual_pool_and_wire_projection(self):
  code=receipt_fixture_code().replace('struct Publication{','struct Publication{own_return_gates:[u32;8],').replace('Publication{calls:','Publication{own_return_gates:[0;8],calls:')
  code=code.replace('struct Display{','struct Display{wires:RefCell<Vec<(usize,usize,Vec<u32>)>>,').replace('Display{real:unsafe','Display{wires:RefCell::new(vec![]),real:unsafe')
  code=code.replace('impl Display{','impl Display{fn wire_row(&self,n:usize,line:usize,v:&[(u32,usize)]){assert!((20..=27).contains(&n)&&line<4);self.wires.borrow_mut().push((n,line,v.iter().map(|(v,_)|*v).collect()));self.real.wire_row(n,line,v);}')
  code=code.replace('struct ModemOwner{','struct ModemOwner{headers:Option<Wire>,').replace('impl ModemOwner{','impl ModemOwner{fn registry_wire_headers(&self)->Option<Wire>{self.headers}')
  code=code.replace('struct Free{at:Option<u64>,slot:u8,cause:u8,empty_mask:u8}','struct Free{at:Option<u64>,slot:u8,cause:u8,empty_mask:u8,original:Slot}')
  code=code.replace('struct Pool{first_free:Option<Free>,admitted_slot:Option<u8>,admitted:Option<u64>}','struct Pool{first:Snapshot,latest:Snapshot,failures:u32,finished:bool,first_free:Option<Free>,admitted_slot:Option<u8>,admitted:Option<u64>,admitted_empty:u8}')
  code=code.replace('#[path=PANEL]mod panel;',r'''
#[derive(Clone,Copy,Default,PartialEq)]struct Slot{kind:u8,flags:u8,op:u16,source:Addr,destination:Addr,remote:u32,len:u16,sent:u16,transaction:u16,message:u16}
#[derive(Clone,Copy,Default,PartialEq)]struct Addr{port:u32}
#[derive(Default)]struct Snapshot{at:Option<u64>,slots:[Slot;8]}
#[derive(Clone,Copy,Default)]struct Header{at:Option<u64>,tx:bool,command:u16,channel:u16,parameter:u32,bytes:u16,qrtr:Option<[u32;3]>,qmi:Option<(u8,u16,u16)>,del_client:Option<[u32;2]>,tftp:Option<[u32;2]>}
#[derive(Clone,Copy)]struct Wire{headers:[Option<Header>;79],completion:Option<Header>,omitted:u32,unparsed:u32,finished:bool}
impl Default for Wire{fn default()->Self{Self{headers:[None;79],completion:None,omitted:0,unparsed:0,finished:false}}}
#[path=PANEL]mod panel;
''')
  code+=r'''
 o.tx3_fifo=Some(Fifo{peer_tail_at:Some(140),completion:Some(Bounds{last_absent:Some(180),first_visible:Some(200),bytes_ahead:Some(104)}),finished:true,history_evictions:1});
 o.elf_fifo=Some(ElfFifo{remote_intent:0,armed_at:Some(321),unread_at_arm:48,core_done_at:Some(390),fifo:Fifo{peer_tail_at:Some(330),completion:Some(Bounds{last_absent:Some(350),first_visible:Some(370),bytes_ahead:Some(160)}),finished:true,history_evictions:0}});
 let first=core::array::from_fn(|i|Slot{kind:4,flags:0x3b,op:9,source:Addr{port:0x4002},destination:Addr{port:9},remote:i as u32,len:88,sent:88,transaction:3+i as u16,message:0x21});let mut latest=first;latest[6].destination.port=0x49;
 o.pool=Some(Pool{first:Snapshot{at:Some(400),slots:first},latest:Snapshot{at:Some(500),slots:latest},failures:4,finished:true,first_free:Some(Free{at:Some(600),slot:2,cause:1,empty_mask:4,original:first[2]}),admitted_slot:Some(2),admitted:Some(700),admitted_empty:0});
 let h=|i|Header{at:Some(1000+i as u64),command:9,channel:2,parameter:i,bytes:88,qrtr:Some([1,0x5000,0x49]),tftp:Some([0x60000|i,39]),..Header::default()};
 let mut w=Wire::default();w.headers=core::array::from_fn(|i|Some(h(i as u32)));w.completion=Some(Header{command:14,parameter:7,at:Some(1200),..Header::default()});w.omitted=23;w.unparsed=2;w.finished=true;o.headers=Some(w);
 let values=|n|d.rows.borrow().iter().find(|(i,_)|*i==n).unwrap().1.clone();
 r.show_registry_receipt_fifo(&d,&o);assert_eq!(*d.title.borrow(),"ELF POOL RX TAIL");assert_eq!(d.rows.borrow().len(),18);
 assert_eq!(values(2),[0,0,u32::MAX,6,3,0x3fa4]);assert_eq!(values(3),[20,30,40,110,120,130]);assert_eq!(values(4),[200,210,220,290,230,2]);
 assert_eq!(values(5),[40,80,100,104,1,1]);assert_eq!(values(6),[230,250,270,160,1,0]);assert_eq!(values(7),[300,400,4,64,600,1]);assert_eq!(values(8),[500,0x20104,600,2,0,21]);
 assert_eq!(values(9),[0x043b0009,0x4002,9,2,0x580058,0x50021]);assert_eq!(values(10),[79,23,2,48,1,1]);
 for i in 0..8{let t=latest[i];assert_eq!(values(11+i),[0x043b0009,t.source.port,t.destination.port,t.remote,0x580058,(u32::from(t.transaction)<<16)|0x21]);}
 {let wire=d.wires.borrow();assert_eq!(wire.len(),32);let ids:Vec<_>=wire.iter().map(|(_,_,v)|v[4]).collect();let expected:Vec<_>=(0..4).chain(52..79).chain([7]).collect();assert_eq!(ids,expected);assert_eq!((wire[31].0,wire[31].1),(27,3));assert_eq!(wire[0].2[9..],[10,0x60000,39]);}
 assert_eq!(values(28),[1,0x7fffffff,0,0,0x35,3]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_POOL_WIRE_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 // A real no-pool slow path retains original priority and own-return gates.
 let saved=o.pool.take();o.window.predecessor.as_mut().unwrap().publication.as_mut().unwrap().own_return_gates=[0,19,44,33,0,0,32,0];d.rows.borrow_mut().clear();d.wires.borrow_mut().clear();r.show_registry_receipt_fifo(&d,&o);assert_eq!(values(11),[0,19,44,33]);assert_eq!(values(12),[0,0,32,0]);assert_eq!(values(13),[0;6]);assert_eq!(values(18),[0,1,2,3,4,5]);o.pool=saved;
 // Every partial history length, including no opening, has no duplicates or stale completion.
 for n in 0..=79{let mut w=Wire::default();for i in 0..n{w.headers[i]=Some(h(i as u32));}o.headers=Some(w);d.rows.borrow_mut().clear();d.wires.borrow_mut().clear();r.show_registry_receipt_fifo(&d,&o);
  let ids:Vec<_>=d.wires.borrow().iter().map(|(_,_,v)|v[4]).collect();let start=n.min(4).max(n.saturating_sub(27));let expect:Vec<_>=(0..n).filter(|i|*i<n.min(4)||*i>=start).map(|i|i as u32).collect();assert_eq!(ids,expect);assert_eq!(ids.len(),n.min(31));assert_eq!(values(10)[3],(n-ids.len())as u32);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);}
 let mut w=Wire::default();w.headers[0]=Some(Header{del_client:Some([0,0x49]),qmi:Some((2,3,0x21)),..h(1)});w.headers[1]=Some(Header{tftp:None,del_client:Some([0,0x49]),..h(2)});w.headers[2]=Some(Header{tftp:None,qmi:Some((2,3,0x21)),..h(3)});w.headers[3]=Some(Header{tftp:None,qrtr:None,..h(4)});o.headers=Some(w);d.rows.borrow_mut().clear();d.wires.borrow_mut().clear();r.hz=0;r.show_registry_receipt_fifo(&d,&o);
 {let v=d.wires.borrow();assert_eq!(v[0].2[9..],[10,0x60001,39]);assert_eq!(v[1].2[9..],[13,0,0x49]);assert_eq!(v[2].2[9..],[2,3,0x21]);assert_eq!(v[3].2[6..],[u32::MAX,u32::MAX,u32::MAX,15,u32::MAX,u32::MAX]);assert!(v.iter().all(|(_,_,v)|v[0]==u32::MAX));}
 o=ModemOwner::default();d.rows.borrow_mut().clear();d.wires.borrow_mut().clear();r.hz=1000000;r.show_registry_receipt_fifo(&d,&o);assert_eq!(values(7),[u32::MAX,u32::MAX,0,u32::MAX,u32::MAX,0]);assert_eq!(values(9),[u32::MAX;6]);assert_eq!(values(11),[u32::MAX;4]);assert_eq!(values(13),[u32::MAX;6]);assert_eq!(values(18),[0;6]);assert_eq!(values(10),[0,u32::MAX,u32::MAX,0,0,0]);assert!(d.wires.borrow().is_empty());assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual pool/wire panel: all8 owners, exact freed owner, changed mask, sample bounds,32 bounded headers, all80 history lengths, tags, absent/clockless data and both physical buffers passed");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_receipt_fifo.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-registry-tx3-wire-headers"','--cfg','feature="sunfish-elf-pool-wire-panel"','--cfg','feature="sunfish-tx3-credit-return-turn"','--cfg','feature="sunfish-tx3-own-return-turn"','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code','-Aunreachable_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
