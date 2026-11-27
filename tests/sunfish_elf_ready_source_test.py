from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='4123cf54'
def without_elf_ready(path,s):
 s=re.sub(r'/\* ELF_READY_BEGIN \*/\n.*?/\* ELF_READY_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-elf-ready-grant-turn = ["sunfish-elf-digest-outcome"]\n','')
 return s
class Boundary(unittest.TestCase):
 def test_parent_source_recovered(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_elf_ready(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_bound_to_live_request_not_observation(self):
  s=(ROOT/'rust/src/qcom_glink_qmi.rs').read_text();blocks=re.findall(r'/\* ELF_READY_BEGIN \*/\n(.*?)/\* ELF_READY_END \*/',s,flags=re.S);self.assertEqual(len(blocks),4)
  s=''.join(blocks)
  for bad in ['elf_window','elf_exchange','registry_window','intent_ack_clock','rmtfs_event_clock','unsafe','with_io']:self.assertNotIn(bad,s)
  self.assertIn('self.request_qmi(handle)',s);self.assertIn('q.service_name==b"kernel/elf_loader" && q.domain_offset==0',s);self.assertIn('self.elf_ready_grant_bound=true',s)
if __name__=='__main__':unittest.main()
