from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* MCFG_IDLE_DETAIL_BEGIN \*/\n.*?/\* MCFG_IDLE_DETAIL_END \*/\n'
COUNTS={'rust/src/qcom_glink_fifo.rs':5,'rust/src/sunfish_mpss_linux_transport.rs':1,'rust/src/qcom_owner_mcfg_idle.rs':3,'rust/src/qcom_fifo_mcfg_idle.rs':3,'rust/src/qcom_fifo_mcfg_rx_veto.rs':1,'tests/sunfish_glink_fifo_test.rs':3}
def without_detail(path,source):
 blocks=re.findall(PATTERN,source,re.S)
 if path in COUNTS:
  assert len(blocks)==COUNTS[path],(path,len(blocks));assert all('#[cfg(feature="sunfish-mcfg-idle-detail")]'in b for b in blocks)
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S)
class DetailBoundary(unittest.TestCase):
 def test_exact_disabled_source_and_all_protocol_boundaries(self):
  for name in ['qcom_glink_fifo.rs','sunfish_mpss_linux_transport.rs','qcom_owner_mcfg_idle.rs','qcom_fifo_mcfg_idle.rs','qcom_fifo_mcfg_rx_veto.rs','qcom_modem_owner.rs','qcom_glink_qmi.rs','qcom_qrtr_flow.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_tftp_server_check.rs','qcom_owner_mcfg_close.rs']:
   p='rust/src/'+name;self.assertEqual(without_detail(p,(ROOT/p).read_text()),subprocess.check_output(['git','show','32ebe7a0:'+p],text=True),p)
  p='tests/sunfish_glink_fifo_test.rs';self.assertEqual(without_detail(p,(ROOT/p).read_text()),subprocess.check_output(['git','show','32ebe7a0:'+p],text=True))
 def test_no_new_fifo_access_or_selection(self):
  s=(ROOT/'rust/src/qcom_fifo_mcfg_idle_detail.rs').read_text()
  for word in ['.sample(','.publish_next(','.read_rx_at(','.ingest(','.queue_tftp_response(','.finish_tftp_request(']:self.assertNotIn(word,s)
  self.assertIn('copy_within(5..8,4)',s);self.assertIn('if self.idle_detail.captured{return;}',s)
if __name__=='__main__':unittest.main()
