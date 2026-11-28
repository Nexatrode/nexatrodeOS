from pathlib import Path
import ast,json,subprocess,tempfile,unittest
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
class Panel(unittest.TestCase):
 def test_actual_ruleset_fifo_renderer(self):
  tree=ast.parse((ROOT/'tests/sunfish_ready_startup_outcome_test.py').read_text());code=next(n.value for n in ast.walk(tree)if isinstance(n,ast.Constant)and isinstance(n.value,str)and 'fn main(){'in n.value);code=code[:code.index(' let values=|n|')]
  code=code.replace('struct Read{','struct Read{dispatched:Option<u64>,seq:Option<u64>,timeout_ms:u32,request_bytes:u32,options_present:u32,options_flags:u32,rsize:u32,reply_bytes:u32,local_port:u32,attempts:u32,waits:[u32;2],').replace('result:i as u32});','result:i as u32,..Read::default()});')
  code=code.replace('reads:[Option<Read>;4]','reads:[Option<Read>;8]')
  code=code.replace('struct ModemOwner{','struct ModemOwner{rw:[Option<RW>;2],ef:Option<EF>,rv:RV,').replace('impl ModemOwner{','impl ModemOwner{fn ruleset_reply_fifo(&self,n:usize)->Option<RW>{self.rw.get(n).copied().flatten()}fn elf_reply_fifo(&self)->Option<EF>{self.ef}fn ruleset_handoff_trace(&self)->RV{self.rv}')
  code=code.replace('#[path=PANEL]mod panel;',r'''
#[derive(Clone,Copy,Default)]struct RV{calls:u32,outcome:u32,flags:u32,writer:WV}
#[derive(Clone,Copy,Default)]struct WV{calls:u32}
#[derive(Clone,Copy,Default)]struct RX{last_absent:Option<u64>,first_visible:Option<u64>,bytes_ahead:Option<u32>,visible_at_close:Option<bool>}
#[derive(Clone,Copy,Default)]struct FF{peer_tail_at:Option<u64>,completion:Option<RX>,finished:bool,history_evictions:u32}
#[derive(Clone,Copy,Default)]struct RW{record:u64,intent:u32,armed:Option<u64>,pending_tx_bytes:u32,unread_at_arm:u32,unread_at_close:Option<u32>,fifo:FF}
#[derive(Clone,Copy,Default)]struct EF{core_done_at:Option<u64>,fifo:FF}
impl Read{fn record_sequence(&self)->Option<u64>{self.seq}}
#[path=PANEL]mod panel;
''')
  code+=r'''
 for i in 0..8{let t=400+i as u64*10;o.early.reads[i]=Some(Read{peer:Address{port:0x20+i as u32},flags:0xbf,seq:Some(0x10000000a+i as u64),timeout_ms:1000,request_bytes:85,options_present:0x17,options_flags:0,rsize:4,reply_bytes:19,local_port:0x5001+i as u32,opcode:5,number:1,attempts:2,waits:[1,0],received:Some(t),dispatched:Some(t+1),queued:Some(t+2),published:Some(t+3),done:Some(t+9),close:Some(t+8),client:u32::MAX,..Read::default()});}
 o.early.total=[4,2];o.early.omitted=[0,0];o.rv=RV{calls:1,outcome:2,flags:0x5f,writer:WV{calls:1}};
 o.rw=[Some(RW{record:0x10000000b,intent:0x31,armed:Some(414),pending_tx_bytes:0,unread_at_arm:8,unread_at_close:Some(72),fifo:FF{peer_tail_at:Some(415),completion:Some(RX{last_absent:Some(430),first_visible:Some(440),bytes_ahead:Some(112),visible_at_close:Some(false)}),finished:true,history_evictions:0}}),Some(RW{record:0x100000010,intent:0x32,armed:Some(464),pending_tx_bytes:0,unread_at_arm:16,unread_at_close:None,fifo:FF{peer_tail_at:Some(465),completion:Some(RX::default()),finished:true,history_evictions:8}})];
 o.ef=Some(EF{core_done_at:Some(380),fifo:FF{peer_tail_at:Some(350),completion:Some(RX{last_absent:Some(360),first_visible:Some(370),bytes_ahead:Some(40),visible_at_close:None}),..FF::default()}});
 let values=|n|d.rows.borrow().iter().find(|(i,_)|*i==n).unwrap().1.clone();r.show_ruleset_reply_fifo(&d,&o);assert_eq!(*d.title.borrow(),"RULESET REPLY FIFO");assert_eq!(d.rows.borrow().len(),27);
 assert_eq!(values(2),[0,0,u32::MAX,6,4,0]);assert_eq!(values(3),[200,210,220,230,1,3]);assert_eq!(values(4),[1,2,0x5f,1,3,0x3fa4]);assert_eq!(values(5),[0x21,0xbf,0xb,1,0x31]);assert_eq!(values(6),[310,311,312,313,319,318]);assert_eq!(values(7),[314,315,0,8,72]);assert_eq!(values(8),[330,340,112,0,1,0]);assert_eq!(values(9),[0x50001,0x5002,19,2,1,0]);assert_eq!(values(10),[85,0x17,0,4,1000,u32::MAX]);
 assert_eq!(values(11),[0x26,0xbf,0x10,1,0x32]);assert_eq!(values(12),[360,361,362,363,369,368]);assert_eq!(values(13),[364,365,0,16,u32::MAX]);assert_eq!(values(14),[u32::MAX,u32::MAX,u32::MAX,u32::MAX,1,8]);assert_eq!(values(17),[0x20,0xbf,0xa,1,0x50001]);assert_eq!(values(19)[0],0x27);assert_eq!(values(21)[0],0x22);assert_eq!(values(23)[0],0x23);assert_eq!(values(25),[0xcf,1,2,3,1,0x35]);assert_eq!(values(26),[250,260,270,40,280,230]);assert_eq!(values(27),[0x1f,15,19,21,25]);assert_eq!(values(28),[1,0x7fffffff,0,0,1,1]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_RULESET_FIFO_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 o.rw[0].as_mut().unwrap().record=999;d.rows.borrow_mut().clear();r.show_ruleset_reply_fifo(&d,&o);assert_eq!(values(5)[4],u32::MAX);assert_eq!(values(7),[u32::MAX;5]);assert_eq!(values(8),[u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,u32::MAX]);
 o.rw[0].as_mut().unwrap().record=0x10000000b;r.hz=0;d.rows.borrow_mut().clear();r.show_ruleset_reply_fifo(&d,&o);assert_eq!(values(6),[u32::MAX;6]);assert_eq!(values(7),[u32::MAX,u32::MAX,0,8,72]);assert_eq!(values(8),[u32::MAX,u32::MAX,112,0,1,0]);
 r.hz=1000000;o=ModemOwner::default();d.rows.borrow_mut().clear();r.show_ruleset_reply_fifo(&d,&o);assert_eq!(values(5),[u32::MAX,0,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(values(9),[u32::MAX,u32::MAX,u32::MAX,0,0,0]);assert_eq!(values(7),[u32::MAX;5]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);println!("Actual ruleset FIFO panel: both identities, publication/read/visibility/close bounds, evictions, absent/clockless fields, no mismatched watch join, all rows and both buffers passed");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_ruleset_reply_fifo.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code);subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True);subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
