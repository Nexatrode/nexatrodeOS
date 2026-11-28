from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='cf680e21'
def without_credit_return(path,s):
 from sunfish_elf_pool_wire_source_test import without_pool_wire
 s=without_pool_wire(path,s)
 s=re.sub(r'/\* TX3_CREDIT_BEGIN \*/\n.*?/\* TX3_CREDIT_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-tx3-credit-return-turn = ["sunfish-tx3-own-return-turn"]\n','')
 if path=='rust/src/qcom_tx3_own_return.rs':
  s=s.replace('&& !tx.registry_grant_turn_used && (!tx.registry_credit_turn_used\n                )','&& !tx.registry_grant_turn_used && !tx.registry_credit_turn_used')
 return s
class Boundary(unittest.TestCase):
 def test_exact_parent_source_recovered(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_credit_return(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_credit_policy_uses_private_live_state_and_normal_selectors(self):
  s=(ROOT/'rust/src/qcom_tx3_own_return.rs').read_text().split('/* TX3_CREDIT_BEGIN */')[-1]
  for bad in ['registry_publication','registry_window','elf_window','intent_ack_clock','rmtfs_event_clock','with_io','unsafe']:self.assertNotIn(bad,s)
  self.assertIn('self.next_transmit_in_order([0])',s);self.assertIn('self.next_transmit_in_order([1])',s);self.assertIn('tx.tx3_own_credit_committed',s)
if __name__=='__main__':unittest.main()
