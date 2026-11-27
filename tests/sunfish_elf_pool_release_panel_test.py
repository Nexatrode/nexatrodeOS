"""Run the actual selected renderer against retained first/latest slot identities."""
from pathlib import Path
import ast, json, os, subprocess, tempfile, unittest
from sunfish_tx3_elf_retry_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
BASE='6f1d64bc'

def without_pool(path,source):
 import re
 from sunfish_startup_pressure_source_test import without_startup
 source=without_startup(path,source)
 if path=='rust/src/pixel_mpss_tx3_elf_retry.rs':
  pat=r'/\* ELF_POOL_PANEL_BEGIN \*/\n.*?/\* ELF_POOL_PANEL_END \*/\n'
  blocks=re.findall(pat,source,re.S);assert len(blocks)==2
  assert all('#[cfg(feature="sunfish-elf-pool-release")]'in b for b in blocks)
  source=re.sub(pat,'',source,flags=re.S)
 if path=='rust/Cargo.toml':source=source.replace('sunfish-elf-pool-release = ["sunfish-tx3-elf-retry"]\n','')
 return source

class PoolPanel(unittest.TestCase):
 def test_only_rendering_changes(self):
  files=subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']
  for file in files:
   self.assertEqual(without_pool(file,(ROOT/file).read_text()),subprocess.check_output(['git','show',BASE+':'+file],text=True),file)
  s=(ROOT/'rust/src/pixel_mpss_elf_pool_release.rs').read_text()
  for bad in ['&mut','unsafe','with_io','revalidate','queue_reply','publish_next','clock()']:self.assertNotIn(bad,s)
 def test_actual_selected_panel(self):
  tree=ast.parse((ROOT/'tests/sunfish_tx3_elf_retry_panel_test.py').read_text())
  code=next(n.value for n in ast.walk(tree) if isinstance(n,ast.Constant) and isinstance(n.value,str) and 'fn main(){'in n.value)
  code=code[:code.index(' r.show_tx3_elf_retry(&d,&o);')]+r'''
 r.show_tx3_elf_retry(&d,&o);
 assert_eq!(values(2),[0,0,1,u32::MAX,u32::MAX,6]);
 assert_eq!(values(3),[35,40,610,30,50,65]);assert_eq!(values(4),[20,800,860,900]);
 assert_eq!(values(5),[200,600,5,2,800,1]);assert_eq!(values(6),[650,0x10102,0,0,1,15]);
 for i in 0..8{let a=o.pool.as_ref().unwrap().first.slots[i];let b=o.pool.as_ref().unwrap().latest.slots[i];
  for (n,s)in [(7+i*2,a),(8+i*2,b)]{
   assert_eq!(values(n),[(u32::from(s.kind)<<24)|(u32::from(s.flags)<<16)|u32::from(s.op),s.source.port,s.destination.port,s.remote,(u32::from(s.len)<<16)|u32::from(s.sent),(u32::from(s.transaction)<<16)|u32::from(s.message)]);
  }
 }
 assert_ne!(values(9),values(10));assert_eq!(values(23),values(9));
 assert_eq!(values(24),[1,0,1,700,1]);assert_eq!(values(25),[60,85,90,1,1,2]);assert_eq!(values(26),[0,1,7,2,3,4]);
 assert_eq!(values(27),[2,17,0xac,2,2,0xa0]);assert_eq!(values(28),[1,0x7fffffff,0,0,15,0x1017f]);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_ELF_POOL_RELEASE_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 r.hz=0;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(values(6),[u32::MAX,0x10102,0,0,1,15]);assert_eq!(values(23),values(9));
 r.hz=1000000;o.pool.as_mut().unwrap().first_free=None;o.pool.as_mut().unwrap().admitted_slot=None;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(values(23),[u32::MAX;6]);assert_eq!(values(6),[u32::MAX,u32::MAX,u32::MAX,0,1,15]);
 o.pool=None;o.old.predecessor=None;o.old.target=None;o.trace.elf=None;o.trace.txn4=None;d.rows.borrow_mut().clear();r.show_tx3_elf_retry(&d,&o);assert_eq!(values(5),[u32::MAX,u32::MAX,0,u32::MAX,u32::MAX,0]);assert_eq!(values(7),[u32::MAX;6]);assert_eq!(values(22),[u32::MAX;6]);assert_eq!(values(27),[0,u32::MAX,u32::MAX,0,u32::MAX,u32::MAX]);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("First/latest eight slots, exact opening freed owner, independent full/free/admission snapshots, clockless and absent captures, six-word rows and both framebuffers passed");
}
'''
  code=code.replace('message:u16,source:Address','message:u16,qrtr_type:u32,source:Address').replace('message:0xffff,source:Address','message:0xffff,qrtr_type:1,source:Address')
  code=code.replace('struct Capacity{flags:u32,result:u8}', 'struct Capacity{flags:u32,result:u8,armed:Option<u64>,entered:Option<u64>,ended:Option<u64>,free_slot:Option<u8>,arms:u32,full_skips:u32}')
  code=code.replace('Capacity{flags:15,result:1}', 'Capacity{flags:15,result:1,armed:Some(160),entered:Some(185),ended:Some(190),free_slot:Some(1),arms:1,full_skips:2}')
  runtime=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text()
  code=code.replace('#[path=DISPLAY]','#[path='+json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))+']').replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('CLOCK',function(runtime,'time_from_us')).replace('#[path=PANEL]','#[path='+json.dumps(str(ROOT/'rust/src/pixel_mpss_tx3_elf_retry.rs'))+']')
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code)
   subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-elf-pool-release"','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code','-Aunreachable_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True)
   subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
