from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='98a3b4b3'
def without_elf_credit(path,s):
 s=re.sub(r'/\* ELF_CREDIT_BEGIN \*/\n.*?/\* ELF_CREDIT_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-elf-credit-pair-turn = ["sunfish-elf-ready-grant-turn"]\n','')
 if path=='rust/src/qcom_glink_qmi.rs':s=s.replace('&& (!self.transmit[*i].registry_credit_turn_used\n                ))}else{None};','&& !self.transmit[*i].registry_credit_turn_used)}else{None};').rstrip()+'\n'
 return s
class Boundary(unittest.TestCase):
 def test_parent_production_recovered(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_elf_credit(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_private_state_and_normal_credit_selector(self):
  s=(ROOT/'rust/src/qcom_glink_qmi.rs').read_text();start=s.index('    fn elf_ready_credit_can_continue(');s=s[start:]
  for bad in ['elf_window','elf_exchange','registry_window','intent_ack_clock','rmtfs_event_clock','unsafe','with_io','registry_publication']:self.assertNotIn(bad,s)
  self.assertIn('self.next_transmit_in_order([1])',s);self.assertIn('bound!=h',s)
if __name__=='__main__':unittest.main()
