"""Actual selected dual-buffer display switches on a real retained pool capture."""
from pathlib import Path
import ast,json,os,subprocess,tempfile,unittest
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
class Panel(unittest.TestCase):
 def test_actual_adaptive_renderer(self):
  tree=ast.parse((ROOT/'tests/sunfish_tx3_elf_retry_panel_test.py').read_text())
  code=next(n.value for n in ast.walk(tree)if isinstance(n,ast.Constant)and isinstance(n.value,str)and'fn main(){'in n.value)
  code=code[:code.index(' r.show_tx3_elf_retry(&d,&o);')]+r'''
 let pool=o.pool.take();
 r.show_tx3_elf_retry(&d,&o);assert_eq!(d.title.borrow().as_deref(),Some("STARTUP REPLY PRESSURE"));
 assert_eq!(values(3),[35,40,610,2,3,88]);assert_eq!(values(4),[17,0xac,42,30,40,45]);assert_eq!(values(5),[0,1,12,0]);assert_eq!(values(6),[0,0,1,1]);assert_eq!(values(7),[0,1,2,3]);assert_eq!(values(8),[4,5,6,7]);assert_eq!(values(9),[8,9,10,11]);assert_eq!(values(10),[1,2,3,4,7]);
 assert_eq!(values(13),[0x1ff,4,610,3,6]);assert_eq!(values(14),[700,750,760,770]);assert_eq!(values(16),[50,10,10,30]);assert_eq!(values(17),[0,1,7,2,3,4]);assert_eq!(values(18),[9,5,0x11223344,0x55667788,0xaabbccdd,0x12345678]);assert_eq!(values(19),[600,0x502,2,1]);assert_eq!(values(20),[0x33b04,7,0x43b04,8]);assert_eq!(values(24),[800,8,1,0,4,7]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_STARTUP_REPLY_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 // Counts alone never select the pool view; the typed retained capture does.
 o.trace.elf.as_mut().unwrap().waits_by_reason[2]=99;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(d.title.borrow().as_deref(),Some("STARTUP REPLY PRESSURE"));assert_eq!(values(17)[2],99);
 o.pool=pool;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(d.title.borrow().as_deref(),Some("STARTUP POOL PRESSURE"));
 assert_eq!(values(5),[200,600,5,2,800,1]);assert_eq!(values(6),[650,0x10102,0,0,1,15]);
 for i in 0..8{for(n,s)in[(7+i*2,o.pool.as_ref().unwrap().first.slots[i]),(8+i*2,o.pool.as_ref().unwrap().latest.slots[i])]{assert_eq!(values(n),[(u32::from(s.kind)<<24)|(u32::from(s.flags)<<16)|u32::from(s.op),s.source.port,s.destination.port,s.remote,(u32::from(s.len)<<16)|u32::from(s.sent),(u32::from(s.transaction)<<16)|u32::from(s.message)]);}}
 assert_ne!(values(9),values(10));assert_eq!(values(23),values(9));assert_eq!(values(24),[1,0,1,700,1]);assert_eq!(values(25),[60,85,90,1,1,2]);assert_eq!(values(27),[2,17,0xac,2,2,0xa0]);assert_eq!(values(28),[1,0x7fffffff,0,0,15,0x1017f]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_STARTUP_POOL_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 // Retained pool identity selects this view even when its clocks are absent.
 r.hz=0;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(d.title.borrow().as_deref(),Some("STARTUP POOL PRESSURE"));assert_eq!(values(6),[u32::MAX,0x10102,0,0,1,15]);assert_eq!(values(23),values(9));
 o.pool=None;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(d.title.borrow().as_deref(),Some("STARTUP REPLY PRESSURE"));assert_eq!(values(3),[u32::MAX,u32::MAX,u32::MAX,2,3,88]);assert_eq!(values(13),[0x1ff,4,u32::MAX,3,6]);assert_eq!(values(16),[u32::MAX;4]);
 r.hz=1000000;o.old.predecessor=None;o.old.target=None;o.trace.elf=None;o.window=None;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(values(3),[u32::MAX,u32::MAX,u32::MAX,0,u32::MAX,u32::MAX]);assert_eq!(values(19),[u32::MAX;4]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual adaptive renderer: reply gates/retry boundaries without a pool, all eight first/latest records with a pool, independent clockless selection, missing data, row widths and both physical buffers passed");
}
'''
  code=code.replace('struct Display{real:physical::Display,','struct Display{title:RefCell<Option<String>>,real:physical::Display,').replace('fn replace_line(&self,n:usize,s:&str){','fn replace_line(&self,n:usize,s:&str){if n==1{*self.title.borrow_mut()=Some(s.to_owned());}').replace('let d=Display{real:','let d=Display{title:RefCell::new(None),real:')
  code=code.replace('message:u16,source:Address','message:u16,qrtr_type:u32,source:Address').replace('message:0xffff,source:Address','message:0xffff,qrtr_type:1,source:Address')
  code=code.replace('struct Capacity{flags:u32,result:u8}','struct Capacity{flags:u32,result:u8,armed:Option<u64>,entered:Option<u64>,ended:Option<u64>,free_slot:Option<u8>,arms:u32,full_skips:u32}').replace('Capacity{flags:15,result:1}','Capacity{flags:15,result:1,armed:Some(160),entered:Some(185),ended:Some(190),free_slot:Some(1),arms:1,full_skips:2}')
  ready=bool(os.getenv('SUNFISH_READY_HANDOFF_PANEL'))
  if ready:
   code=code.replace('.iter().find(|(row,second,_)|','.iter().rev().find(|(row,second,_)|')
   code+='\nstruct ReadyHandoff{flags:u32,arms:u32,receipts:u32,services:u32,result:u32}\nimpl ModemOwner{fn ready_handoff_trace(&self)->ReadyHandoff{ReadyHandoff{flags:31,arms:1,receipts:1,services:1,result:1}}}\n'
   code=code.replace('STARTUP REPLY PRESSURE','REGISTRY READY HANDOFF').replace('STARTUP POOL PRESSURE','READY HANDOFF POOL')
   code=code.replace('assert_eq!(values(13),[0x1ff,4,610,3,6]);','assert_eq!(values(26),[31,1,1,1,1]);assert_eq!(values(13),[0x1ff,4,610,3,6]);')
  runtime=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text()
  code=code.replace('#[path=DISPLAY]','#[path='+json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))+']').replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('CLOCK',function(runtime,'time_from_us')).replace('#[path=PANEL]','#[path='+json.dumps(str(ROOT/'rust/src/pixel_mpss_tx3_elf_retry.rs'))+']')
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024',*[x for f in (['sunfish-registry-ready-handoffs']if ready else [])+['sunfish-startup-pressure','sunfish-elf-pool-release','sunfish-wrq-post-return-callers','sunfish-ipa-registration-reply-headers']for x in ['--cfg',f'feature="{f}"']],'-Adead_code','-Aunreachable_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
