from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* STARTUP_PREREQ_BEGIN \*/\n.*?/\* STARTUP_PREREQ_END \*/\n'
COUNTS={'rust/src/qcom_glink_qmi.rs':9,'rust/src/qcom_glink_fifo.rs':1,'rust/src/qcom_modem_owner.rs':2}
def without_startup(path,source):
 from sunfish_tftp_arrival_order_source_test import without_arrival
 source=without_arrival(path,source)
 blocks=re.findall(PATTERN,source,re.S)
 if path in COUNTS:assert len(blocks)==COUNTS[path],(path,len(blocks));assert all('#[cfg(feature="sunfish-startup-prerequisites")]'in b for b in blocks)
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S)
class Boundary(unittest.TestCase):
 def test_exact_observer_only_additions(self):
  for n in ['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_modem_owner.rs','qcom_elf_publication.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_mcfg_close.rs','qcom_owner_mcfg_close.rs','qcom_tftp_server_check.rs','qcom_qrtr_flow.rs','sunfish_mpss_linux_transport.rs']:
   f='rust/src/'+n;self.assertEqual(without_startup(f,(ROOT/f).read_text()),subprocess.check_output(['git','show','6c3bfeb2:'+f],text=True),f)
 def test_no_observer_io_or_scheduling(self):
  s=(ROOT/'rust/src/qcom_startup_prerequisites.rs').read_text()
  for n in ['.publish_next(','.queue_tftp_response(','.read_rx_at(','.revalidate(','.sample(','.ingest(']:self.assertNotIn(n,s)
  self.assertIn('r.record==t.reference_flow',s);self.assertIn('r.remote_intent==id',s)
if __name__=='__main__':unittest.main()
