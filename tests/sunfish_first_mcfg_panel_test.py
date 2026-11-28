from pathlib import Path
import ast,json,os,subprocess,tempfile,unittest
from sunfish_post_elf_mcfg_panel_test import function
ROOT=Path(__file__).resolve().parents[1]
class Panel(unittest.TestCase):
 def test_actual_first_response_panel(self):
  tree=ast.parse((ROOT/'tests/sunfish_post_elf_outcome_panel_test.py').read_text())
  code=next(n.value for n in ast.walk(tree)if isinstance(n,ast.Constant)and isinstance(n.value,str)and 'fn main(){'in n.value)
  code=code[:code.index(' let values=|n|')]
  code=code.replace('struct Display{','struct Display{title:RefCell<String>,').replace('Display{real:unsafe','Display{title:RefCell::new(String::new()),real:unsafe').replace('self.real.replace_line(n,s);','if n==1{*self.title.borrow_mut()=s.into();}self.real.replace_line(n,s);').replace('s.len()<=32','s.len()<=57').replace('sum::<usize>()<=64','sum::<usize>()<=57')
  code=code.replace('struct ModemOwner{','struct ModemOwner{old:Option<Prior>,fifo:Option<Fifo>,').replace('let mut o=ModemOwner{','let mut o=ModemOwner{old:Some(Prior{queued_at:Some(110),published_at:Some(120),done_at:Some(130),publication:Some(Pub{calls:5,mask:0x7e23ac})}),fifo:Some(Fifo{remote_intent:8,core_done_at:Some(225)}),')
  code=code.replace('impl ModemOwner{','impl ModemOwner{fn registry_window_trace(&self)->Window{Window{predecessor:self.old}}fn elf_reply_fifo(&self)->Option<Fifo>{self.fifo}')
  code=code.replace('#[path=PANEL]mod panel;',r'''
#[derive(Clone,Copy)]struct Pub{calls:u32,mask:u32}
#[derive(Clone,Copy)]struct Prior{queued_at:Option<u64>,published_at:Option<u64>,done_at:Option<u64>,publication:Option<Pub>}
struct Window{predecessor:Option<Prior>}
#[derive(Clone,Copy)]struct Fifo{remote_intent:u32,core_done_at:Option<u64>}
#[path=PANEL]mod panel;
''')
  code+=r'''
 let values=|n|d.rows.borrow().iter().rev().find(|(i,s,_)|*i==n&&*s).unwrap().2.iter().map(|(v,_)|*v).collect::<Vec<_>>();
 r.show_first_mcfg_response(&d,&o);assert_eq!(*d.title.borrow(),"FIRST MCFG RESPONSE");assert_eq!(d.rows.borrow().len(),35);
 assert_eq!(values(2),[0,0,u32::MAX,6,5,0x7e23ac]);assert_eq!(values(3),[10,20,30,100,120,130]);assert_eq!(values(4),[1,7,8,125,130,50]);assert_eq!(values(5),[0x4a,255,42,1,73,1]);assert_eq!(values(6),[140,145,150,160,170,190]);assert_eq!(values(7),[0,6,u32::MAX,45,0x5005,0x31]);
 assert_eq!(values(8),[31,0,0,u32::MAX,0x50009,0x50009]);assert_eq!(values(9),[7680,1000,10,0,0,u32::MAX]);assert_eq!(values(10),[15,0,512,1000,1,1]);assert_eq!(values(11),[u32::MAX,u32::MAX,u32::MAX,u32::MAX,180,180]);assert_eq!(values(12),[0,1,2,20,8,u32::MAX]);assert_eq!(values(13),[146,150,4,3,4]);assert_eq!(values(14),[1,6,5,3,52,52]);assert_eq!(values(15),[149,150,29,0,10,2]);assert_eq!(values(16),[151,152,0x830003,6,9,1]);assert_eq!(values(17),[1,2,3,4,40,1]);assert_eq!(values(18),[8,0x55,93,1,24]);assert_eq!(values(19),[1,1,0,2,2]);assert_eq!(values(28),[1,0x7fffffff,0,0,u32::MAX,0]);
 let rows=d.rows.borrow();let js:Vec<_>=rows.iter().filter(|(n,_,_)|(20..=27).contains(n)).collect();assert_eq!(js.len(),16);for(i,h)in js.iter().enumerate(){assert_eq!(h.2[0].0,200+i as u32);assert_eq!(h.2[5].0,13);}drop(rows);
 assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 if let Ok(p)=std::env::var("SUNFISH_FIRST_MCFG_PPM"){let mut b=b"P6\n1080 2340\n255\n".to_vec();for v in &pixels[..1080*2340]{b.extend([(v>>16)as u8,(v>>8)as u8,*v as u8]);}std::fs::write(p,b).unwrap();}
 {let m=o.cap.as_mut().unwrap().mcfg.as_mut().unwrap();m.flags=3;m.file_before=None;m.server_result=Some(17);m.reply_opcode=u32::MAX;m.reply_number=u32::MAX;m.reply_bytes=0;m.admission=Admission::default();}d.rows.borrow_mut().clear();r.show_first_mcfg_response(&d,&o);assert_eq!(values(5)[5],u32::MAX);assert_eq!(values(7)[..4],[17,u32::MAX,u32::MAX,0]);assert_eq!(values(16),[u32::MAX;6]);
 r.hz=0;d.rows.borrow_mut().clear();r.show_first_mcfg_response(&d,&o);assert_eq!(values(3),[u32::MAX;6]);assert_eq!(values(8)[4..],[0x50009,0x50009]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 r.hz=1000000;o.cap=None;o.old=None;o.fifo=None;o.trace.elf=None;o.trace.elf_exchange=None;d.rows.borrow_mut().clear();r.show_first_mcfg_response(&d,&o);assert_eq!(values(2)[4..],[u32::MAX;2]);assert_eq!(values(5),[u32::MAX,0,u32::MAX,u32::MAX,u32::MAX,u32::MAX]);assert_eq!(values(9),[u32::MAX;6]);assert_eq!(values(18),[u32::MAX,u32::MAX,u32::MAX,u32::MAX,0]);assert_eq!(values(28),[1,0x7fffffff,0,0,u32::MAX,0]);assert_eq!(&pixels[..1080*2340],&pixels[1080*2340..]);
 println!("Actual first-MCFG panel: exact record and first request, options including zero, response/ending, admission/writer/RX context, bounded16 journal, ELF/IPA/WLAN, absent/clockless fields and both physical buffers passed");
}
'''
  code=code.replace('DISPLAY',json.dumps(str(ROOT/'rust/src/pixel_probe_display.rs'))).replace('CLOCK',function((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'time_from_us')).replace('PACKED',function((ROOT/'rust/src/pixel_mpss_runtime_diagnostic.rs').read_text(),'packed_counts')).replace('PANEL',json.dumps(str(ROOT/'rust/src/pixel_mpss_first_mcfg_response.rs')))
  with tempfile.TemporaryDirectory()as tmp:
   p=Path(tmp);(p/'panel.rs').write_text(code);subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--cfg','feature="sunfish-wrq-post-return-callers"','--cfg','feature="sunfish-ipa-registration-reply-headers"','-Adead_code',str(p/'panel.rs'),'-o',str(p/'panel')],check=True);subprocess.run([str(p/'panel')],check=True)
if __name__=='__main__':unittest.main()
