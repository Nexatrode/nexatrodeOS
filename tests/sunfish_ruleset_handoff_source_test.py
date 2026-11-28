from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='09893b10'
def without_ruleset(path,s):
 s=re.sub(r'/\* RULESET_HANDOFF_BEGIN \*/\n.*?/\* RULESET_HANDOFF_END \*/\n','',s,flags=re.S)
 if path=='rust/Cargo.toml':s=s.replace('sunfish-ruleset-service-handoff = ["sunfish-startup-reply-coverage"]\n','')
 if path=='rust/src/pixel_mpss_startup_reply_coverage.rs':
  s=s.replace('  {let f=owner.elf_reply_fifo();','  let f=owner.elf_reply_fifo();').replace('age(e.and_then(|e|e.done))]);}\n  let ipa=', 'age(e.and_then(|e|e.done))]);\n  let ipa=')
 return s
class Boundary(unittest.TestCase):
 def test_exact_parent_outside_scoped_handoff(self):
  for f in subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']:
   self.assertEqual(without_ruleset(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_real_policy_gates_and_ordinary_writer(self):
  s=(ROOT/'rust/src/qcom_owner_ruleset_handoff.rs').read_text()
  for part in ['self.check()?','s.service_used','s.closed','!self.efs_error_turn_used','self.has_pending_tftp_reply()','self.pending_tftp_request()!=Some(handle)','self.pending_mcfg_close.is_some()','self.pending_firmware_close.is_some()','self.ruleset_handoff.service_used=true;','self.service_combined_tftp_step','self.service_tftp_server_check_step']:self.assertIn(part,s)
  self.assertLess(s.index('self.ruleset_handoff.service_used=true;'),s.index('let result='))
  for bad in ['startup_prerequisites','intent_ack_clock','rmtfs_event_clock','queue_traced','read_rx_at','finish_after_glink','unsafe']:self.assertNotIn(bad,s)
  w=(ROOT/'rust/src/qcom_fifo_ruleset_handoff.rs').read_text()
  for part in ['for i in 0..4','self.mcfg_guard=true;let result=self.publish_next(g);self.mcfg_guard=false;','read_notification_sent','self.tftp_rx_snapshot()','Err(error)=>{t.stop=6;return Err(error);}','if !matches!(op,Published::ReceiveDone(_)|Published::ReceiveIntent(_)|Published::IntentRequestAck{..})']:self.assertIn(part,w)
  for bad in ['session.publish','queue_tftp','read_rx_at','remote_for','unsafe','clock()']:self.assertNotIn(bad,w)
if __name__=='__main__':unittest.main()
