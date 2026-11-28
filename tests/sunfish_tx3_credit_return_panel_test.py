from pathlib import Path
import json,os,subprocess,tempfile,unittest
from sunfish_registry_receipt_fifo_panel_test import receipt_fixture_code
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
class Panel(unittest.TestCase):
 def test_actual_credit_return_panel(self):
  code=receipt_fixture_code().replace('struct Publication{','struct Publication{own_return_gates:[u32;8],').replace('Publication{calls:','Publication{own_return_gates:[0;8],calls:')
  code+=r'''
 o.window.predecessor.as_mut().unwrap().publication.as_mut().unwrap().own_return_gates=[0,1,2,3,4,5,6,7];
 o.window.predecessor.as_mut().unwrap().publication.as_mut().unwrap().mask=0x7e03a4;
 let values=|n|d.rows.borrow().iter().find(|(i,_)|*i==n).unwrap().1.clone();
 r.show_registry_receipt_fifo(&d,&o);assert_eq!(*d.title.borrow(),"TX3 CREDIT RETURN");assert_eq!(d.rows.borrow().len(),27);
 assert_eq!(values(9),[0,1,2,3]);assert_eq!(values(25),[4,5,6,7]);assert_eq!(values(5)[2],0x7e03a4);assert_eq!(values(6),[u32::MAX,u32::MAX,u32::MAX,u32::MAX,0,u32::MAX]);assert_eq!(values(24),[33,1,34,1,3,1]);assert_eq!(values(28),[1,0x7fffffff,0,0,1,0]);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_TX3_CREDIT_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 o=ModemOwner::default();d.rows.borrow_mut().clear();r.show_registry_receipt_fifo(&d,&o);assert_eq!(values(9),[u32::MAX;4]);assert_eq!(values(25),[u32::MAX;4]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual TX3 credit-return panel passed: all8 first-failing counters, actual commit mask, retained receipt/digest/status, missing captures and both physical buffers");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_receipt_fifo.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-tx3-credit-return-turn"','--cfg','feature="sunfish-tx3-own-return-turn"','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
