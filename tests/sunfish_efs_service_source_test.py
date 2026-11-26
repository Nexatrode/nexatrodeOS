from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* EFS_SERVICE_HANDOFF_BEGIN \*/\n.*?/\* EFS_SERVICE_HANDOFF_END \*/\n'
COUNTS={'rust/src/qcom_modem_owner.rs':6,'rust/src/pixel_mpss_runtime.rs':2,'rust/src/sunfish_ipa_linux_qmi_owner.rs':1}
def without_service(path,source):
 blocks=re.findall(PATTERN,source,re.S)
 if path in COUNTS:assert len(blocks)==COUNTS[path],(path,len(blocks));assert all('#[cfg(feature="sunfish-efs-service-handoff")]'in b for b in blocks)
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S)
class Boundary(unittest.TestCase):
 def test_disabled_source_and_unchanged_checks(self):
  for n in ['qcom_modem_owner.rs','pixel_mpss_runtime.rs','sunfish_ipa_linux_qmi_owner.rs','qcom_glink_fifo.rs','qcom_glink_qmi.rs','qcom_qrtr_flow.rs','qcom_owner_efs_early.rs','qcom_fifo_efs_early.rs','qcom_startup_prerequisites.rs','qcom_tftp_server_check.rs','qcom_owner_mcfg_close.rs','pixel_mpss_runtime_irq.rs']:
   f='rust/src/'+n;self.assertEqual(without_service(f,(ROOT/f).read_text()),subprocess.check_output(['git','show','4f294dae:'+f],text=True),f)
 def test_bounded_actual_handles_and_native_mux(self):
  s=(ROOT/'rust/src/qcom_owner_efs_service.rs').read_text();r=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text();o=(ROOT/'rust/src/qcom_modem_owner.rs').read_text()
  for gate in ['self.first_efs_error_request','!self.efs_ruleset_queued','self.first_efs_error_closed','self.has_pending_tftp_reply()','self.pending_tftp_request() != Some(handle)','self.pending_mcfg_close.is_some()','self.pending_firmware_close.is_some()']:self.assertIn(gate,s)
  self.assertLess(s.index('self.efs_service_used = true;'),s.index('self.service_combined_tftp_step'))
  self.assertNotIn('startup_prerequisites',s);self.assertNotIn('publish_next',s);self.assertNotIn('loop',s);self.assertIn('if self.efs_ruleset_request==Some(request)',o)
  self.assertIn('Some(&mut self.firmware)',r);self.assertLess(r.index('match tftp_progress.map_err'),r.index('owner.service_efs_handoff'))
if __name__=='__main__':unittest.main()
