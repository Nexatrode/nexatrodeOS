"""Explicit opt-in bounded scheduling; disabled paths preserve the prior source."""
from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* MCFG_IDLE_TURN_BEGIN \*/\n.*?/\* MCFG_IDLE_TURN_END \*/\n'
COUNTS={'rust/src/qcom_glink_fifo.rs':5,'rust/src/sunfish_mpss_linux_transport.rs':1,'rust/src/qcom_fifo_mcfg_queued.rs':4,'rust/src/qcom_modem_owner.rs':7}
def without_idle(path,source):
 from sunfish_mcfg_idle_detail_source_test import without_detail
 source=without_detail(path,source)
 blocks=re.findall(PATTERN,source,re.S)
 if path in COUNTS:
  assert len(blocks)==COUNTS[path],(path,len(blocks));assert all('#[cfg(feature="sunfish-mcfg-idle-turn")]'in b for b in blocks)
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S)
class IdleBoundary(unittest.TestCase):
 def test_exact_disabled_source_and_unchanged_core(self):
  for n in ['qcom_glink_fifo.rs','sunfish_mpss_linux_transport.rs','qcom_fifo_mcfg_queued.rs','qcom_modem_owner.rs','qcom_owner_mcfg_queued.rs','qcom_fifo_mcfg_rx_veto.rs','qcom_glink_qmi.rs','qcom_qrtr_flow.rs','qcom_tftp_server_check.rs','qcom_owner_mcfg_close.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs']:
   p='rust/src/'+n;self.assertEqual(without_idle(p,(ROOT/p).read_text()),subprocess.check_output(['git','show','8dfeedce:'+p],text=True),p)
 def test_authoritative_retained_scope_and_hard_budget(self):
  owner=(ROOT/'rust/src/qcom_owner_mcfg_idle.rs').read_text();fifo=(ROOT/'rust/src/qcom_fifo_mcfg_idle.rs').read_text();runtime=(ROOT/'rust/src/qcom_modem_owner.rs').read_text()
  for n in ['r.mcfg_idle_used','MISSING_REPLY','mcfg_close_transfers','self.peer_tftp_replies[slot].as_mut().unwrap().mcfg_idle_used=true;']:self.assertIn(n,owner)
  self.assertIn('for _ in 0..4',fifo);self.assertIn('self.publish_next(g)',fifo);self.assertIn('matches!(rx,Receive::Idle)',runtime);self.assertIn('!mcfg_idle_spent',runtime)
  for n in ['post_elf_mcfg','record_sequence','first_server_check_clock']:self.assertNotIn(n,owner)
if __name__=='__main__':unittest.main()
