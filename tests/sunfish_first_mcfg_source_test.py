from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='2ff2b35d'
def without_first_mcfg(path,s):
 from sunfish_startup_coverage_source_test import without_coverage
 s=without_coverage(path,s)
 s=re.sub(r'/\* FIRST_MCFG_BEGIN \*/\n.*?/\* FIRST_MCFG_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':
  s=s.replace('sunfish-first-mcfg-response = ["sunfish-elf-pool-wire-panel"]\n','')
  s=s.replace('utm-maps-vault-key = ["utm-maps-private", "utm-wrapped-vault-experiment"]\n','')
  s=s.replace('utm-maps-session-lock = ["utm-maps-vault-key", "utm-dynamic-vault-lock"]\n','')
  s=s.replace('utm-maps-session-lock = ["utm-maps-private", "utm-dynamic-vault-lock"]\n','')
 # Independent mainline Maps overlay, exact known source only; absent in this native profile.
 if path in ['rust/src/vault_crypto.rs','rust/src/vault_disk.rs']:
  before=subprocess.check_output(['git','show',BASE+':'+path],text=True)
  after=subprocess.check_output(['git','show','dc18d26e:'+path],text=True)
  previous=subprocess.check_output(['git','show','a524912e:'+path],text=True)
  assert s in [before,previous,after], 'unreviewed Maps/vault source delta'
  s=before
 if path=='rust/src/qcom_post_elf_mcfg.rs':s=s.replace('if (after\n            ) && cap.mcfg.is_none()', 'if after && cap.mcfg.is_none()')
 return s
class Boundary(unittest.TestCase):
 def test_exact_parent_execution_source_except_binding_and_renderer(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_first_mcfg(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_binding_has_one_observation_condition_and_no_new_policy(self):
  s=(ROOT/'rust/src/qcom_post_elf_mcfg.rs').read_text();blocks=re.findall(r'/\* FIRST_MCFG_BEGIN \*/\n(.*?)/\* FIRST_MCFG_END \*/',s,re.S)
  self.assertEqual(blocks,['            || cfg!(feature="sunfish-first-mcfg-response")\n'])
  p=(ROOT/'rust/src/pixel_mpss_first_mcfg_response.rs').read_text()
  for bad in ['&mut','unsafe','with_io','queue_reply','publish_next','read_rx_at','clock()']:self.assertNotIn(bad,p)
if __name__=='__main__':unittest.main()
