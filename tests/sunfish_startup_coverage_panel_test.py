from pathlib import Path
import ast,json,os,subprocess,tempfile,unittest
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
class Panel(unittest.TestCase):
 def test_actual_coverage_renderer(self):
  tree=ast.parse((ROOT/'tests/sunfish_ready_startup_outcome_test.py').read_text());code=next(n.value for n in ast.walk(tree)if isinstance(n,ast.Constant)and isinstance(n.value,str)and 'fn main(){'in n.value)
  code=code[:code.index(' let values=|n|')]
  code=code.replace('struct Read{','struct Read{timeout_ms:u32,local_port:u32,request_bytes:u32,options_present:u32,options_flags:u32,rsize:u32,attempts:u32,waits:[u32;2],seq:Option<u64>,dispatched:Option<u64>,')
  code=code.replace('struct Early{reads:[Option<Read>;4],total:', 'struct Early{reads:[Option<Read>;8],digest_total:u32,digest_omitted:u32,total:')
  code=code.replace('result:i as u32});','result:i as u32,..Read::default()});')
  code=code.replace('impl ModemOwner{','impl ModemOwner{fn elf_reply_fifo(&self)->Option<Fifo>{Some(Fifo{fifo:Bounds{peer_tail_at:Some(350),completion:Some(Receipt{last_absent:Some(360),first_visible:Some(370),bytes_ahead:Some(96)})},core_done_at:Some(380)})}')
  code=code.replace('#[path=PANEL]mod panel;', '''
mod diagnostic{pub(super) PACKED}
#[derive(Clone,Copy)]struct Fifo{fifo:Bounds,core_done_at:Option<u64>}
#[derive(Clone,Copy)]struct Bounds{peer_tail_at:Option<u64>,completion:Option<Receipt>}
#[derive(Clone,Copy)]struct Receipt{last_absent:Option<u64>,first_visible:Option<u64>,bytes_ahead:Option<u32>}
impl Read{fn record_sequence(&self)->Option<u64>{self.seq}}
#[path=PANEL]mod panel;''')
  code+=r'''
 for(i,index)in [0,1,6,7,2,3].iter().enumerate(){let t=400+i as u64*10;o.early.reads[*index]=Some(Read{peer:Address{port:0x21+i as u32},flags:0xbf,seq:Some(0x100000020+i as u64),timeout_ms:if i==2{0}else{1000},local_port:0x5002+i as u32,request_bytes:85,rsize:0,options_present:0x17,options_flags:2,opcode:5,number:1,attempts:3,waits:[1,2],received:Some(t),dispatched:Some(t+1),queued:Some(t+2),published:Some(t+3),done:Some(t+4),close:Some(t+5),..Read::default()});}
 o.early.total=[5,2];o.early.omitted=[1,0];o.early.digest_total=3;o.early.digest_omitted=1;
 let values=|n|d.rows.borrow().iter().find(|(i,_)|*i==n).unwrap().1.clone();r.show_startup_reply_coverage(&d,&o);
 assert_eq!(*d.title.borrow(),"EARLY REPLY COVERAGE");assert_eq!(d.rows.borrow().len(),27);
 assert_eq!(values(2),[0,0,u32::MAX,6,3,0x3fa4]);assert_eq!(values(3),[0x1f,5,10,15,19,21]);assert_eq!(values(4),[25,200,210,220,230,3]);assert_eq!(values(5),[20,30,40,110,120,130]);assert_eq!(values(6),[5,1,2,0,3,1]);
 for i in 0..6{assert_eq!(values(7+i*3),[0x21+i as u32,0xbf,0x20+i as u32,1,if i==2{0}else{1000},0x5002+i as u32]);let t=300+i as u32*10;assert_eq!(values(8+i*3),[t,t+1,t+2,t+3,t+4,t+5]);assert_eq!(values(9+i*3),[85,0x20017,0,0x50001,3,0x10002]);}
 assert_eq!(values(25),[250,260,270,96,280,230]);assert_eq!(values(26),[0xcf,1,2,3,1,0x35]);assert_eq!(values(27),[1,1,0,2,2,8]);assert_eq!(values(28),[1,0x7fffffff,0,0,1,1]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_COVERAGE_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 r.hz=0;d.rows.borrow_mut().clear();r.show_startup_reply_coverage(&d,&o);assert_eq!(values(8),[u32::MAX;6]);assert_eq!(values(13)[2..],[0x22,1,0,0x5004]);
 r.hz=1000000;o=ModemOwner::default();d.rows.borrow_mut().clear();r.show_startup_reply_coverage(&d,&o);assert_eq!(values(7),[u32::MAX,0,u32::MAX,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(values(9),[u32::MAX,0,u32::MAX,u32::MAX,0,0]);assert_eq!(values(26),[u32::MAX;6]);assert_eq!(values(6),[0;6]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual coverage renderer: six distinct identities/order, numeric option presence and explicit zero, every clock, omissions, current IPA and both physical buffers passed");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_startup_reply_coverage.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code);subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True);subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
