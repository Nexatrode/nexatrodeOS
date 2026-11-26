from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* EFS_EARLY_TURN_BEGIN \*/\n.*?/\* EFS_EARLY_TURN_END \*/\n'
COUNTS={'rust/src/qcom_glink_fifo.rs':4,'rust/src/sunfish_mpss_linux_transport.rs':1,'rust/src/qcom_modem_owner.rs':7,'rust/src/pixel_mpss_runtime.rs':1,'rust/src/sunfish_ipa_linux_qmi_owner.rs':1}
def without_early(path,source):
 blocks=re.findall(PATTERN,source,re.S)
 if path in COUNTS:assert len(blocks)==COUNTS[path],(path,len(blocks));assert all('#[cfg(feature="sunfish-efs-early-error-turn")]'in b for b in blocks)
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S)
class Boundary(unittest.TestCase):
 def test_exact_disabled_source_and_unchanged_core(self):
  for n in ['qcom_glink_fifo.rs','sunfish_mpss_linux_transport.rs','qcom_modem_owner.rs','qcom_startup_prerequisites.rs','pixel_mpss_runtime.rs','qcom_glink_qmi.rs','qcom_qrtr_flow.rs','qcom_elf_publication.rs','qcom_mcfg_close.rs','qcom_owner_mcfg_close.rs','pixel_mpss_runtime_irq.rs','qcom_tftp_server_check.rs','sunfish_ipa_linux_qmi_owner.rs']:
   f='rust/src/'+n;self.assertEqual(without_early(f,(ROOT/f).read_text()),subprocess.check_output(['git','show','5fed6630:'+f],text=True),f)
 def test_bound_scope_and_checked_writer(self):
  s=(ROOT/'rust/src/qcom_fifo_efs_early.rs').read_text();o=(ROOT/'rust/src/qcom_modem_owner.rs').read_text();c=(ROOT/'rust/src/qcom_startup_prerequisites.rs').read_text()
  for n in ['for i in 0..4','self.mcfg_guard=true','self.publish_next(g)','self.mcfg_guard=false','Transmit::Blocked','tftp_rx_snapshot()']:self.assertIn(n,s)
  self.assertIn('self.first_efs_error_request==Some((reply.request,reply.meta.route.destination))',o);self.assertIn('!self.first_efs_error_closed',o);self.assertNotIn('first_efs_error_candidate',c);self.assertIn('self.efs_error_turn_used=true',o)
if __name__=='__main__':unittest.main()
