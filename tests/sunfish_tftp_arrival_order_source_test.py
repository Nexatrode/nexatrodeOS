from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* TFTP_ARRIVAL_ORDER_BEGIN \*/\n.*?/\* TFTP_ARRIVAL_ORDER_END \*/\n'
def without_arrival(path,source):
 blocks=re.findall(PATTERN,source,re.S)
 if path=='rust/src/qcom_modem_owner.rs':assert len(blocks)==1 and '#[cfg(feature="sunfish-tftp-arrival-order")]'in blocks[0],(path,len(blocks))
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S)
class Boundary(unittest.TestCase):
 def test_exact_disabled_source_and_unchanged_transport(self):
  for n in ['qcom_modem_owner.rs','qcom_glink_fifo.rs','qcom_glink_qmi.rs','qcom_elf_publication.rs','qcom_qrtr_flow.rs','qcom_mcfg_close.rs','qcom_owner_mcfg_close.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_tftp_server_check.rs','sunfish_mpss_linux_transport.rs']:
   f='rust/src/'+n;self.assertEqual(without_arrival(f,(ROOT/f).read_text()),subprocess.check_output(['git','show','4f8805e1:'+f],text=True),f)
 def test_bounded_host_handles_only_after_coalescing(self):
  s=(ROOT/'rust/src/qcom_modem_owner.rs').read_text();b=re.findall(PATTERN,s,re.S)[0]
  self.assertIn('for index in 0..MAX_REQUESTS',b);self.assertIn('.take()',b)
  for n in ['adapter','publish','revalidate','unsafe','clock','queue_tftp']:self.assertNotIn(n,b)
  self.assertLess(s.index('if coalesce {'),s.index(b));self.assertLess(s.index(b),s.index('let Some(slot) = self.tftp_requests.iter_mut().find('))
if __name__=='__main__':unittest.main()
