from pathlib import Path
import ast,json,subprocess,tempfile,unittest
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
class Panel(unittest.TestCase):
 def test_actual_control_renderer(self):
  tree=ast.parse((ROOT/'tests/sunfish_ready_startup_outcome_test.py').read_text());code=next(n.value for n in ast.walk(tree)if isinstance(n,ast.Constant)and isinstance(n.value,str)and 'fn main(){'in n.value);code=code[:code.index(' let values=|n|')]
  code=code.replace('struct Read{','struct Read{seq:Option<u64>,').replace('result:i as u32});','result:i as u32,..Read::default()});').replace('reads:[Option<Read>;4]','reads:[Option<Read>;8]')
  code=code.replace('struct Display{','struct Display{wires:RefCell<Vec<(usize,usize,Vec<u32>)>>,').replace('Display{real:unsafe','Display{wires:RefCell::new(vec![]),real:unsafe')
  code=code.replace('impl Display{','impl Display{fn wire_row(&self,n:usize,line:usize,v:&[(u32,usize)]){assert!((8..=23).contains(&n)&&line<4);self.wires.borrow_mut().push((n,line,v.iter().map(|(v,_)|*v).collect()));self.real.wire_row(n,line,v);}')
  code=code.replace('struct ModemOwner{','struct ModemOwner{control:Control,rw:[Option<RW>;2],ef:Option<EF>,').replace('impl ModemOwner{','impl ModemOwner{fn ruleset_control_trace(&self)->&Control{&self.control}fn ruleset_reply_fifo(&self,n:usize)->Option<RW>{self.rw.get(n).copied().flatten()}fn elf_reply_fifo(&self)->Option<EF>{self.ef}')
  code=code.replace('#[path=PANEL]mod panel;',r'''
#[derive(Clone,Copy,Default)]struct RX{last_absent:Option<u64>,first_visible:Option<u64>,bytes_ahead:Option<u32>,visible_at_close:Option<bool>}
#[derive(Clone,Copy,Default)]struct FF{peer_tail_at:Option<u64>,completion:Option<RX>}
#[derive(Clone,Copy,Default)]struct RW{record:u64,intent:u32,fifo:FF}
#[derive(Clone,Copy,Default)]struct EF{fifo:FF}
#[derive(Clone,Copy,Default)]struct Req{record:Option<u64>,bounds:Option<RX>}
#[derive(Clone,Copy,Default)]struct Entry{at:Option<u64>,flags:u8,command:u16,channel:u16,parameter:u32,bytes:u16,kind:u32,a:u32,b:u32,tag:u8,c:u32,d:u32}
struct Control{entries:[Option<Entry>;64],requests:[Option<Req>;2],total:u32,omitted:u32,rx:u32,tx:u32,done_mask:u8,unparsed:u32}
impl Default for Control{fn default()->Self{Self{entries:[None;64],requests:[None;2],total:0,omitted:0,rx:0,tx:0,done_mask:0,unparsed:0}}}
impl Read{fn record_sequence(&self)->Option<u64>{self.seq}}
#[path=PANEL]mod panel;
''')
  code+=r'''
 for i in 0..8{let t=400+i as u64*10;o.early.reads[i]=Some(Read{peer:Address{port:0x20+i as u32},seq:Some(0x10000000a+i as u64),received:Some(t),published:Some(t+3),done:Some(t+9),close:Some(t+8),..Read::default()});}
 o.early.total=[4,2];o.early.omitted=[0,0];o.control.total=72;o.control.omitted=8;o.control.rx=40;o.control.tx=32;o.control.done_mask=3;
 for n in 0..2{let seq=[0x10000000b,0x100000010][n];o.control.requests[n]=Some(Req{record:Some(seq),bounds:Some(RX{last_absent:Some(390+n as u64),first_visible:Some(400+n as u64),..RX::default()})});o.rw[n]=Some(RW{record:seq,intent:0x31+n as u32,fifo:FF{peer_tail_at:Some(420+n as u64),completion:Some(RX{first_visible:Some(440+n as u64),..RX::default()})}});}
 for n in 0..64{o.control.entries[n]=Some(Entry{at:Some(1000+n as u64),flags:(n%3)as u8,command:5,channel:2,parameter:n as u32,bytes:16,kind:u32::MAX,a:u32::MAX,b:u32::MAX,tag:5,c:4096,d:0x50+n as u32});}
 o.ef=Some(EF{fifo:FF{peer_tail_at:Some(350),completion:Some(RX{first_visible:Some(370),..RX::default()})}});
 let values=|n|d.rows.borrow().iter().find(|(i,_)|*i==n).unwrap().1.clone();r.show_ruleset_control_window(&d,&o);assert_eq!(*d.title.borrow(),"RULESET CONTROL WINDOW");assert_eq!(d.rows.borrow().len(),10);
 assert_eq!(values(2),[0,72,8,40,32,3]);assert_eq!(values(3),[0x21,0xb,1,0x31,290,300]);assert_eq!(values(4),[310,313,320,340,319,318]);assert_eq!(values(5),[0x26,0x10,1,0x32,291,301]);assert_eq!(values(6),[360,363,321,341,369,368]);assert_eq!(values(24),[309,308,329,328,339,338]);assert_eq!(values(25),[1,1,2,3,1,0x35]);assert_eq!(values(26),[220,250,270,230,1,3]);assert_eq!(values(27),[21,25,4,1,1,0]);assert_eq!(values(28),[0x7fffffff,0,0,0,3,0]);
 {let w=d.wires.borrow();assert_eq!(w.len(),64);for(n,(row,line,v))in w.iter().enumerate(){assert_eq!((*row,*line),(8+n/4,n%4));assert_eq!(*v,[900+n as u32,(n%3)as u32,5,2,n as u32,16,u32::MAX,u32::MAX,u32::MAX,5,4096,0x50+n as u32]);}}
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_RULESET_CONTROL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 o.control.requests[0].as_mut().unwrap().record=Some(999);o.rw[0].as_mut().unwrap().record=999;d.rows.borrow_mut().clear();d.wires.borrow_mut().clear();r.show_ruleset_control_window(&d,&o);assert_eq!(&values(3)[3..],&[u32::MAX;3]);assert_eq!(&values(4)[2..4],&[u32::MAX;2]);
 r.hz=0;d.rows.borrow_mut().clear();d.wires.borrow_mut().clear();r.show_ruleset_control_window(&d,&o);assert!(d.wires.borrow().iter().all(|(_,_,v)|v[0]==u32::MAX));assert_eq!(values(2),[0,72,8,40,32,3]);assert_eq!(values(5)[3],0x32);
 o=ModemOwner::default();r.hz=1000000;d.rows.borrow_mut().clear();d.wires.borrow_mut().clear();r.show_ruleset_control_window(&d,&o);assert!(d.wires.borrow().is_empty());assert_eq!(values(3),[u32::MAX;6]);assert_eq!(values(4),[u32::MAX;6]);assert_eq!(values(2),[0;6]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual control panel: all64 entries, exact original joins, metadata, empty/clockless state, mismatched binding suppression and both physical buffers passed");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_ruleset_control.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code);subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-registry-tx3-wire-headers"','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True);subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
