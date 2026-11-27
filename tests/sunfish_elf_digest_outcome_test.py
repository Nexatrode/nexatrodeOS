"""Bound the exact-name failed-open reply and reuse existing ELF writer traces."""
from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='13f0719f'
def without_digest(path,s):
 from sunfish_elf_ready_source_test import without_elf_ready
 s=without_elf_ready(path,s)
 s=re.sub(r'/\* ELF_DIGEST_BEGIN \*/\n.*?/\* ELF_DIGEST_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-elf-digest-outcome = ["sunfish-ready-startup-outcome"]\n','')
 if path=='rust/src/qcom_tftp_server_check.rs':s=s.replace('        missing && t.mcfg_read.is_none() && t.mcfg_write.is_none()','        t.missing_kind==1 && t.mcfg_read.is_none() && t.mcfg_write.is_none()')
 if path=='rust/src/qcom_startup_prerequisites.rs':
  s=s.replace('  let path=body[2..].split(|b|*b==0).next();\n  let kind=match path{Some(b"/readwrite/ota_firewall/ruleset")=>1,Some(b"/readwrite/modem_efs")=>2,\n  _=>return};','  let path=body[2..].split(|b|*b==0).next();let kind=match path{Some(b"/readwrite/ota_firewall/ruleset")=>1,Some(b"/readwrite/modem_efs")=>2,_=>return};')
  s=s.replace('let c=&mut self.startup_prerequisites;\n  {c.total[k]=c.total[k].saturating_add(1);}','let c=&mut self.startup_prerequisites;c.total[k]=c.total[k].saturating_add(1);').replace('if c.reads[i].is_some(){\n  {c.omitted[k]=c.omitted[k].saturating_add(1);}return;}','if c.reads[i].is_some(){c.omitted[k]=c.omitted[k].saturating_add(1);return;}')
 return s
class Boundary(unittest.TestCase):
 def test_parent_production_delta(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_digest(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_no_new_writer_or_validation_policy(self):
  for f in ['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_modem_owner.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_qrtr_flow.rs']:
   self.assertEqual(without_digest('rust/src/'+f,(ROOT/'rust/src'/f).read_text()),subprocess.check_output(['git','show',BASE+':rust/src/'+f],text=True),f)
  s=(ROOT/'rust/src/pixel_mpss_elf_digest_outcome.rs').read_text()
  for bad in ['unsafe','with_io','publish_next','dispatch_service','intent_ack_clock','rmtfs_event_clock']:self.assertNotIn(bad,s)
if __name__=='__main__':unittest.main()
