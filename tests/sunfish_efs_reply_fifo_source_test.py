from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* EFS_REPLY_FIFO_BEGIN \*/\n.*?/\* EFS_REPLY_FIFO_END \*/\n'
COUNTS={'rust/src/qcom_glink_fifo.rs':10,'rust/src/sunfish_mpss_linux_transport.rs':1,'rust/src/qcom_modem_owner.rs':1}
def without_efs_fifo(path,source):
 blocks=re.findall(PATTERN,source,re.S)
 if path in COUNTS:assert len(blocks)==COUNTS[path],(path,len(blocks));assert all('#[cfg(feature="sunfish-efs-reply-fifo")]'in b for b in blocks)
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S)
class Boundary(unittest.TestCase):
 def test_exact_observer_only_additions(self):
  for n in ['qcom_glink_fifo.rs','sunfish_mpss_linux_transport.rs','qcom_modem_owner.rs','qcom_glink_qmi.rs','qcom_qrtr_flow.rs','qcom_elf_publication.rs','qcom_owner_mcfg_close.rs','qcom_mcfg_close.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_tftp_server_check.rs']:
   f='rust/src/'+n;self.assertEqual(without_efs_fifo(f,(ROOT/f).read_text()),subprocess.check_output(['git','show','b51802bf:'+f],text=True),f)
 def test_no_new_io_or_scheduling(self):
  s=(ROOT/'rust/src/qcom_fifo_efs_reply.rs').read_text()
  for n in ['.sample(','.read_rx_at(','.publish_next(','.queue_tftp_response(','.revalidate(']:
   # The synthetic history tests intentionally call the pure observer sampler.
   self.assertNotIn(n,s[:s.index('#[cfg(test)]')])
  self.assertIn('r.record_sequence()==Some(w.record)',s);self.assertIn('r.remote_intent==w.intent',s)
if __name__=='__main__':unittest.main()
