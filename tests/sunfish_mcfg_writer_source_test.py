"""New writer capture changes observer modules, not transport or runtime policy."""
from pathlib import Path
import subprocess,unittest
from sunfish_mcfg_queued_source_test import legacy_source
ROOT=Path(__file__).resolve().parents[1]
class McfgWriterBoundary(unittest.TestCase):
 def test_complete_protocol_sources_unchanged(self):
  for name in ['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_mcfg_close.rs','qcom_modem_owner.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_mcfg_ram.rs','qcom_tftp_server_check.rs','qcom_qrtr_flow.rs']:
   f='rust/src/'+name;self.assertEqual(legacy_source(f,(ROOT/f).read_text()),subprocess.check_output(['git','show','2672a283:'+f],cwd=ROOT,text=True),f)
 def test_only_void_calls_added_to_existing_elf_observer(self):
  f='rust/src/qcom_elf_publication.rs';s=(ROOT/f).read_text()
  for call in ['begin_mcfg_writer_pass()','mcfg_writer_header(b,result)','end_mcfg_writer_pass(choice)','yield_mcfg_writer_pass()']:
   hook='\n        #[cfg(feature="sunfish-mcfg-writer-trace")]\n        self.'+call+';';self.assertEqual(s.count(hook),1);s=s.replace(hook,'')
  self.assertEqual(s,subprocess.check_output(['git','show','2672a283:'+f],cwd=ROOT,text=True))
if __name__=='__main__':unittest.main()
