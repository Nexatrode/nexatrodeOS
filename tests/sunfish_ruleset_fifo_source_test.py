from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='a310ca9f'
def without_ruleset_fifo(path,s):
 from sunfish_ruleset_control_source_test import without_ruleset_control,without_integrated_maps
 s=without_integrated_maps(path,without_ruleset_control(path,s))
 s=re.sub(r'/\* RULESET_FIFO_BEGIN \*/\n.*?/\* RULESET_FIFO_END \*/\n\n?','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-ruleset-reply-fifo = ["sunfish-ruleset-service-handoff"]\n','')
 return s
class Boundary(unittest.TestCase):
 def test_exact_parent_execution_outside_observation(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_ruleset_fifo(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_new_metadata_has_no_transport_or_policy_authority(self):
  s=(ROOT/'rust/src/qcom_fifo_ruleset_reply.rs').read_text()
  for bad in ['.publish_next(','.queue_tftp','.revalidate(','.sample(','.read_rx_at(','.write_tx(','.ingest(','unsafe','enable_ruleset_service']:self.assertNotIn(bad,s)
  for part in ['[1,6]','!waiting[n]','r.record_sequence()==Some(w.record)','flags&32==0&&r.flags&32!=0','r.remote_intent==w.intent','size==8','word(&self.ring.record,4)==w.intent']:self.assertIn(part,s)
  p=(ROOT/'rust/src/pixel_mpss_ruleset_reply_fifo.rs').read_text()
  for bad in ['&mut','unsafe','with_io','clock()','publish_next','queue_response']:self.assertNotIn(bad,p)
if __name__=='__main__':unittest.main()
