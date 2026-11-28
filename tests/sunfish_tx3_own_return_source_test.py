from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='7dee737a'
def without_own_return(path,s):
 s=re.sub(r'/\* TX3_RETURN_BEGIN \*/\n.*?/\* TX3_RETURN_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-tx3-own-return-turn = ["sunfish-registry-receipt-fifo"]\n','')
 if path=='rust/src/qcom_glink_qmi.rs':s=s.replace('.or(priority_registration_done)\n            .or(denial_done)','.or(priority_registration_done).or(denial_done)')
 return s
class Boundary(unittest.TestCase):
 def test_exact_parent_production_recovered(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_own_return(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_policy_does_not_read_observations(self):
  s=(ROOT/'rust/src/qcom_tx3_own_return.rs').read_text().split('    pub(super) fn observe_tx3_own_return_gate')[0]
  for bad in ['registry_publication','registry_window','elf_window','intent_ack_clock','rmtfs_event_clock','unsafe','with_io']:self.assertNotIn(bad,s)
  self.assertIn('self.next_transmit_in_order([0])',s);self.assertIn('self.next_transmit_in_order([1])',s)
if __name__=='__main__':unittest.main()
