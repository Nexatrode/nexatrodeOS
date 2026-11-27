"""Render slow/clean ELF writer states beside distinct bounded digest transactions."""
from pathlib import Path
import ast,json,os,subprocess,tempfile,unittest
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
def fixture_code():
  tree=ast.parse((ROOT/'tests/sunfish_ready_startup_outcome_test.py').read_text())
  base=next(n.value for n in ast.walk(tree)if isinstance(n,ast.Constant)and isinstance(n.value,str)and'fn main(){'in n.value)
  code=base[:base.index(' let values=|n|')]
  code=code.replace('struct Publication{calls:u32,mask:u32}','struct Publication{calls:u32,mask:u32,transaction:u16,bytes:u16,turn_gates:[u32;8],grant_gates:[u32;12],ack:PubAck}')
  code=code.replace('mask:0x3fa4}','mask:0x3fa4,..Publication::default()}').replace('mask:0x183fa4}','mask:0x183fa4,..Publication::default()}')
  code=code.replace('struct Read{kind:u32,','struct Read{sequence:Option<u64>,reply_bytes:u32,local_port:u32,remote_intent:u32,kind:u32,').replace('result:i as u32});','result:i as u32,..Read::default()});')
  code=code.replace('reads:[Option<Read>;4],total:[u32;2],omitted:[u32;2]','reads:[Option<Read>;6],total:[u32;2],omitted:[u32;2],digest_total:u32,digest_omitted:u32')
  code=code.replace('struct ModemOwner{trace:Trace,','struct ModemOwner{elf_writer:Option<ElfWindow>,turn:Turn,trace:Trace,')
  code=code.replace('impl ModemOwner{','impl ModemOwner{fn elf_window_trace(&self)->Option<&ElfWindow>{self.elf_writer.as_ref()}fn elf_queued_turn_trace(&self)->Turn{self.turn}')
  code=code.replace('#[path=PANEL]mod panel;',r'''
mod diagnostic{pub(super) PACKED}
#[derive(Clone,Copy,Default)]struct PubAck{sent:u32,blocked:u32,done_ahead:u32,ads_ahead:u32}
#[derive(Clone,Copy,Default)]struct WriterRecord{start:Option<u64>,end:Option<u64>,command:u16,kind:u8,flags:u8,transaction:u16,parameter:u32,result:u8}
#[derive(Default)]struct Writer{first:Option<u64>,last:Option<u64>,passes:u32,omitted:u32,total:Option<u64>,max_gap:Option<u64>,publication:Option<Publication>,finished:bool,active:bool,regressed:bool,timed:u32,records:[Option<WriterRecord>;8]}
#[derive(Default)]struct ElfWindow{writer:Writer}
#[derive(Clone,Copy,Default)]struct Turn{flags:u32,published:u32,entry:Option<u64>,end:Option<u64>}
impl Read{fn record_sequence(&self)->Option<u64>{self.sequence}}
#[path=PANEL]mod panel;
''')
  code+=r'''
 o.turn=Turn{flags:31,published:3,entry:Some(311),end:Some(800)};
 let mut writer=Writer{first:Some(340),last:Some(780),passes:11,omitted:3,total:Some(400),max_gap:Some(900),finished:true,timed:11,..Writer::default()};
 writer.publication=Some(Publication{calls:11,mask:0xe0ac,transaction:5,bytes:88,turn_gates:[0,1,9,0,0,0,0,1],grant_gates:core::array::from_fn(|i|i as u32),ack:PubAck{sent:9,blocked:0,done_ahead:1,ads_ahead:1}});
 for i in 0..8{writer.records[i]=Some(WriterRecord{start:Some(340+i as u64*50),end:Some(375+i as u64*50),command:if i==7{9}else{6},kind:if i==7{4}else{0},flags:3,transaction:if i==7{5}else{0xffff},parameter:i as u32,result:1});}
 o.elf_writer=Some(ElfWindow{writer});o.early.digest_total=3;o.early.digest_omitted=1;
 o.early.reads[4]=Some(Read{sequence:Some(0x100000021),reply_bytes:19,local_port:0x5009,remote_intent:3,kind:3,peer:Address{port:0x4e},flags:0xbf,opcode:5,number:1,client:u32::MAX,received:Some(900),queued:Some(910),published:Some(920),done:Some(950),close:Some(960),result:0});
 o.early.reads[5]=Some(Read{sequence:Some(0x100000022),reply_bytes:19,local_port:0x500a,remote_intent:u32::MAX,kind:3,peer:Address{port:0x4f},flags:0x8f,opcode:5,number:1,client:u32::MAX,received:Some(970),queued:Some(980),published:None,done:None,close:Some(990),result:0});
 let values=|n|d.rows.borrow().iter().find(|(i,_)|*i==n).unwrap().1.clone();
 r.show_elf_digest_outcome(&d,&o);assert_eq!(*d.title.borrow(),"ELF WRITER DIGEST REPLY");assert_eq!(d.rows.borrow().len(),27);
 assert_eq!(values(4),[200,210,220,230,31,3]);assert_eq!(values(5),[240,680,11,3,400,900]);assert_eq!(values(6),[11,0xe0ac,5,88,1,11]);assert_eq!(values(7),[0,1,9,0]);assert_eq!(values(8),[0,0,0,1]);assert_eq!(values(9),[0,1,2,3]);assert_eq!(values(10),[4,5,6,7]);assert_eq!(values(11),[8,9,10,11]);assert_eq!(values(12),[240,275,0x0603ffff,0,1]);assert_eq!(values(19),[590,625,0x09430005,7,1]);
 assert_eq!(values(20),[0x4e,0xbf,33,1,0]);assert_eq!(values(21),[800,810,820,850,860]);assert_eq!(values(22),[5,1,19,0x5009,3,u32::MAX]);assert_eq!(values(23),[0x4f,0x8f,34,1,0]);assert_eq!(values(24),[870,880,u32::MAX,u32::MAX,890]);assert_eq!(values(26),[9,0,1,1,211,700]);assert_eq!(values(27),[0xcf,0x35,3,3,1,2]);assert_eq!(values(28),[1,0x7fffffff,0,0,21,0x11002e]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_ELF_DIGEST_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 // No second writer pass means gap unavailable, not zero or a hidden stall.
 {let w=&mut o.elf_writer.as_mut().unwrap().writer;w.passes=1;w.timed=1;w.omitted=0;w.max_gap=None;w.records=[None;8];w.active=true;w.finished=false;w.regressed=true;}
 d.rows.borrow_mut().clear();r.show_elf_digest_outcome(&d,&o);assert_eq!(values(5)[5],u32::MAX);assert_eq!(values(6)[4],6);assert_eq!(values(12),[u32::MAX;5]);
 r.hz=0;d.rows.borrow_mut().clear();r.show_elf_digest_outcome(&d,&o);assert_eq!(&values(5)[..2],&[u32::MAX;2]);assert_eq!(values(21),[u32::MAX;5]);assert_eq!(values(20)[2..4],[33,1]);
 r.hz=1000000;o.elf_writer=None;o.early.reads[4]=None;o.early.reads[5]=None;o.trace=Trace::default();o.report=None;d.rows.borrow_mut().clear();r.show_elf_digest_outcome(&d,&o);assert_eq!(values(20),[u32::MAX,0,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(values(22),[u32::MAX,u32::MAX,0,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual ELF/digest panel passed: full gates, first/latest bounded writer records, gap/active/missing clocks, distinct published/canceled digest identities, both physical buffers");
}
'''
  return code

class Panel(unittest.TestCase):
 def test_actual_physical_panel(self):
  code=fixture_code()
  if os.getenv('SUNFISH_ELF_READY_PANEL'):code=code.replace('ELF WRITER DIGEST REPLY','ELF READY GRANT TURN')
  if os.getenv('SUNFISH_ELF_CREDIT_PANEL'):code=code.replace('ELF READY GRANT TURN','ELF CREDIT PAIR TURN')
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_elf_digest_outcome.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024',*(['--cfg','feature="sunfish-elf-credit-pair-turn"']if os.getenv('SUNFISH_ELF_CREDIT_PANEL')else []),*(['--cfg','feature="sunfish-elf-ready-grant-turn"']if os.getenv('SUNFISH_ELF_READY_PANEL')else []),'--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
