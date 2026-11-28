from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='4cdab83e'
def without_receipt(path,s):
 from sunfish_tx3_own_return_source_test import without_own_return
 s=without_own_return(path,s)
 s=re.sub(r'/\* RECEIPT_FIFO_BEGIN \*/\n.*?/\* RECEIPT_FIFO_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-registry-receipt-fifo = ["sunfish-registry-elf-pressure"]\n','')
 return s
def without_maps_delta(path,s):
 # Exact independently integrated mainline overlay; not a Wi-Fi exception.
 if path=='rust/Cargo.toml':s=s.replace('utm-maps-private = []\n','')
 if path=='rust/src/entropy_rng.rs':
  before=subprocess.check_output(['git','show','162a8abd:'+path],text=True)
  after=subprocess.check_output(['git','show','8dc16f91:'+path],text=True)
  assert s==after,'unreviewed entropy source delta'
  return before
 return s
class Boundary(unittest.TestCase):
 def test_parent_source_recovered(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_receipt(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_observer_has_no_policy_or_hardware_access(self):
  s=(ROOT/'rust/src/qcom_fifo_elf_receipt.rs').read_text()
  for bad in ['write_checked','publish_next','with_io','queue_reply','offer_receive','read_rx_at','unsafe','registry_turn_used']:self.assertNotIn(bad,s)
  self.assertIn('Event::ReplyRxDone(h))if *h==w.request',s)
if __name__=='__main__':unittest.main()
