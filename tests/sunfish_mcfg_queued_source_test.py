"""The new bounded path is explicit; disabled configurations preserve all old source."""
from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* MCFG_QUEUED_TURN_BEGIN \*/\n.*?/\* MCFG_QUEUED_TURN_END \*/\n'
COUNTS={'rust/src/qcom_glink_fifo.rs':8,'rust/src/qcom_modem_owner.rs':5,'rust/src/pixel_mpss_runtime.rs':1,'rust/src/sunfish_mpss_linux_transport.rs':1,'rust/src/sunfish_ipa_linux_qmi_owner.rs':1}
def legacy_source(path,source):
 blocks=re.findall(PATTERN,source,re.S)
 if path in COUNTS:
  assert len(blocks)==COUNTS[path],(path,len(blocks))
  assert all('#[cfg(feature="sunfish-mcfg-queued-turn")]' in b for b in blocks)
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S)
class QueuedBoundary(unittest.TestCase):
 def test_exact_disabled_source_and_unchanged_core(self):
  for n in ['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_modem_owner.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_mcfg_close.rs','qcom_owner_mcfg_close.rs','qcom_mcfg_ram.rs','qcom_tftp_server_check.rs','qcom_qrtr_flow.rs','qcom_elf_publication.rs','sunfish_mpss_linux_transport.rs','sunfish_ipa_linux_qmi_owner.rs']:
   p='rust/src/'+n;self.assertEqual(legacy_source(p,(ROOT/p).read_text()),subprocess.check_output(['git','show','a5b582a9:'+p],cwd=ROOT,text=True),p)
 def test_scope_budget_and_fresh_fifo_veto(self):
  owner=(ROOT/'rust/src/qcom_modem_owner.rs').read_text();f=(ROOT/'rust/src/qcom_fifo_mcfg_queued.rs').read_text();adapter=(ROOT/'rust/src/qcom_glink_fifo.rs').read_text()
  self.assertIn('reply.bytes[..reply.meta.bytes]==*crate::qcom_tftp_server_check::MISSING_REPLY',owner);self.assertIn('self.mcfg_close_transfers.iter().flatten().any(',owner);self.assertIn('for _ in 0..4',f);self.assertIn('self.publish_next(generation)',f);self.assertIn('let yield_to_rx=yield_to_rx||mcfg_guard;',adapter);self.assertNotIn('observe_',f)
if __name__=='__main__':unittest.main()
