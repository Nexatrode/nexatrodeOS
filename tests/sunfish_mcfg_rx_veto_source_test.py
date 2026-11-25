"""New hooks only observe already sampled cursors and normally peeked headers."""
from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* MCFG_RX_VETO_BEGIN \*/\n.*?/\* MCFG_RX_VETO_END \*/\n'
COUNTS={'rust/src/qcom_glink_fifo.rs':11,'rust/src/sunfish_mpss_linux_transport.rs':1,'rust/src/qcom_owner_mcfg_queued.rs':1,'rust/src/qcom_fifo_mcfg_queued.rs':1}
def without_rx_veto(path,source):
 blocks=re.findall(PATTERN,source,re.S)
 if path in COUNTS:
  assert len(blocks)==COUNTS[path],(path,len(blocks));assert all('#[cfg(feature="sunfish-mcfg-rx-veto-trace")]'in b for b in blocks)
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S)
class Boundary(unittest.TestCase):
 def test_exact_protocol_and_disabled_source(self):
  for name in ['qcom_glink_fifo.rs','sunfish_mpss_linux_transport.rs','qcom_owner_mcfg_queued.rs','qcom_glink_qmi.rs','qcom_fifo_mcfg_queued.rs','qcom_modem_owner.rs','qcom_qrtr_flow.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_tftp_server_check.rs']:
   p='rust/src/'+name;self.assertEqual(without_rx_veto(p,(ROOT/p).read_text()),subprocess.check_output(['git','show','9bf1ee0c:'+p],text=True),p)
 def test_no_authority_or_new_fifo_access_in_observer(self):
  s=(ROOT/'rust/src/qcom_fifo_mcfg_rx_veto.rs').read_text();self.assertIn('copy_within(5..12,4)',s)
  for forbidden in ['read_rx','write_checked','publish_next','revalidate(','.ingest(','.sample(','.finish_tftp_request(']:self.assertNotIn(forbidden,s)
if __name__=='__main__':unittest.main()
