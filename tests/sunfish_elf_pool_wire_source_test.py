from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='30baf93e'
def without_pool_wire(path,s):
 s=re.sub(r'/\* POOL_WIRE_PANEL_BEGIN \*/\n.*?/\* POOL_WIRE_PANEL_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-elf-pool-wire-panel = ["sunfish-tx3-credit-return-turn"]\n','')
 return s
class Boundary(unittest.TestCase):
 def test_exact_parent_production_source(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_pool_wire(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_new_projection_has_no_policy_or_transport_access(self):
  s=(ROOT/'rust/src/pixel_mpss_elf_pool_wire.rs').read_text()
  for bad in ['&mut','unsafe','with_io','queue_reply','publish_next','clock()','revalidate','read_rx_at']:self.assertNotIn(bad,s)
  self.assertIn('registry_wire_headers()',s);self.assertIn('elf_tx_slots()',s)
if __name__=='__main__':unittest.main()
