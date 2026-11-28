from pathlib import Path
import re,subprocess,unittest
from sunfish_ruleset_handoff_source_test import without_ruleset
ROOT=Path(__file__).resolve().parents[1];BASE='dc18d26e'
def without_coverage(path,s):
 from sunfish_ruleset_handoff_source_test import without_ruleset
 s=without_ruleset(path,s)
 s=re.sub(r'/\* STARTUP_COVERAGE_BEGIN \*/\n.*?/\* STARTUP_COVERAGE_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-startup-reply-coverage = ["sunfish-first-mcfg-response"]\n','')
 if path=='rust/src/qcom_startup_prerequisites.rs':
  s=s.replace('let i=k*2+usize::from(c.reads[k*2].is_some());\n  if c.reads[i].is_some(){','let i=k*2+usize::from(c.reads[k*2].is_some());if c.reads[i].is_some(){')
  s=s.replace('rsize:opts.values[4],\n    result:', 'rsize:opts.values[4],result:')
 return s
class Boundary(unittest.TestCase):
 def test_exact_parent_except_observation_capacity_and_projection(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_coverage(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_no_new_io_clocks_or_policy(self):
  f='rust/src/qcom_startup_prerequisites.rs';a=(ROOT/f).read_text();b=subprocess.check_output(['git','show',BASE+':'+f],text=True)
  self.assertEqual(a.count('intent_ack_clock.map'),b.count('intent_ack_clock.map'))
  for name in ['qcom_glink_fifo.rs','qcom_modem_owner.rs','qcom_glink_qmi.rs','qcom_qrtr_flow.rs','qcom_tftp_server_check.rs','qcom_mcfg_ram.rs','qcom_efs_overlap.rs']:
   path='rust/src/'+name;self.assertEqual(without_ruleset(path,(ROOT/path).read_text()),subprocess.check_output(['git','show',BASE+':'+path],text=True))
  s=(ROOT/'rust/src/pixel_mpss_startup_reply_coverage.rs').read_text()
  for bad in ['&mut','unsafe','clock()','with_io','publish_next','queue_reply','read_rx_at','revalidate']:self.assertNotIn(bad,s)
if __name__=='__main__':unittest.main()
