from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='fb930674'
def without_pressure(path,s):
 from sunfish_registry_receipt_fifo_source_test import without_receipt,without_maps_delta
 s=without_maps_delta(path,without_receipt(path,s))
 s=re.sub(r'/\* REGISTRY_ELF_BEGIN \*/\n.*?/\* REGISTRY_ELF_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-registry-elf-pressure = ["sunfish-elf-credit-pair-turn"]\n','')
 return s
class Boundary(unittest.TestCase):
 def test_entire_parent_production_source_unchanged(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_pressure(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_display_has_no_policy_or_io_authority(self):
  s=(ROOT/'rust/src/pixel_mpss_registry_elf_pressure.rs').read_text()
  for bad in ['unsafe','with_io','publish_next','dispatch_service','rmtfs_event_clock','intent_ack_clock']:self.assertNotIn(bad,s)
  s=(ROOT/'rust/src/pixel_mpss_elf_digest_outcome.rs').read_text();self.assertLess(s.index('self.show_registry_elf_pressure('),s.index('for n in 2..=28'))
if __name__=='__main__':unittest.main()
