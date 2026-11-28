from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='32f0f9ad'
def without_ruleset_control(path,s):
 s=re.sub(r'/\* RULESET_CONTROL_BEGIN \*/\n.*?/\* RULESET_CONTROL_END \*/\n\n?','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-ruleset-control-window = ["sunfish-ruleset-reply-fifo"]\n','')
 return s
# Explicit, independently integrated Maps commit; do not silently exempt a file.
# Only the exact known d26e51c8 text may map to its Wi-Fi-parent e33975ea text.
def without_integrated_maps(path,s):
 if path in ['rust/Cargo.toml','rust/src/entropy_rng.rs','rust/src/vault_disk.rs','rust/src/vault_protocol.rs']:
  known=subprocess.check_output(['git','show','d26e51c8:'+path],text=True)
  if s==known:return subprocess.check_output(['git','show','e33975ea:'+path],text=True)
 return s
class Boundary(unittest.TestCase):
 def test_exact_parent_execution_outside_new_observation(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_ruleset_control(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_observers_have_no_transport_or_policy_authority(self):
  s=(ROOT/'rust/src/qcom_fifo_ruleset_control.rs').read_text()
  for bad in ['.publish_next(','.queue_tftp','.revalidate(','.sample(','.read_rx_at(','.write_tx(','.ingest(','unsafe','enable_ruleset_service']:self.assertNotIn(bad,s)
  for part in ['before[n].is_none()','q.handle==r.observation_handle()','w.binding()==(record,r.remote_intent)','flags&32==0','size==8','self.entries[32..].rotate_left(1)','self.done_mask==3']:self.assertIn(part,s)
  s=(ROOT/'rust/src/pixel_mpss_ruleset_control.rs').read_text()
  for bad in ['&mut','unsafe','with_io','clock()','publish_next','queue_response']:self.assertNotIn(bad,s)
  self.assertIn('q.record.is_some()&&q.record==seq',s);self.assertIn('Some(w.record)==seq',s)
if __name__=='__main__':unittest.main()
