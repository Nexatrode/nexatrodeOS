"""Only the explicit cancellation observation may differ in the cleanup commit."""
from pathlib import Path
import subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
CANCEL_HOOK='                #[cfg(feature="sunfish-mcfg-record-identity")]\n                self.observe_post_mcfg_cancel(i);\n'
def protocol_source(path,source):
 if path=='rust/src/qcom_mcfg_close.rs':
  assert source.count(CANCEL_HOOK)<=1
  return source.replace(CANCEL_HOOK,'')
 return source
class McfgTraceBoundary(unittest.TestCase):
 def test_runtime_transport_and_file_policy_unchanged(self):
  for name in ['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_mcfg_close.rs','qcom_modem_owner.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_mcfg_ram.rs','qcom_tftp_server_check.rs','qcom_qrtr_flow.rs']:
   path='rust/src/'+name;self.assertEqual(protocol_source(path,(ROOT/path).read_text()),subprocess.check_output(['git','show','8f538ca8:'+path],cwd=ROOT,text=True),path)
 def test_hook_is_observation_after_successful_unpublished_cancellation(self):
  source=(ROOT/'rust/src/qcom_mcfg_close.rs').read_text();self.assertEqual(source.count(CANCEL_HOOK),1)
  self.assertLess(source.index('fail_unpublished('),source.index(CANCEL_HOOK));self.assertLess(source.index(CANCEL_HOOK),source.index('self.transmit[i]=Transmit::empty()'))
if __name__=='__main__':unittest.main()
