"""Keep the absent-MCFG close extension outside wire parsing and transport policy."""
from pathlib import Path
import subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
def previous(path):return subprocess.check_output(['git','show','61b35326:'+path],cwd=ROOT,text=True)
def function(source,name):
 a=source.index(name);b=source.index('{',a)+1;depth=1
 while depth:depth+=(source[b]=='{')-(source[b]=='}');b+=1
 return source[a:b]
class MissingCloseScope(unittest.TestCase):
 def test_transport_runtime_and_ram_file_implementation_are_unchanged(self):
  for name in ['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_mcfg_close.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','qcom_qrtr_flow.rs','qcom_mcfg_ram.rs']:
   path='rust/src/'+name;self.assertEqual((ROOT/path).read_text(),previous(path),path)
 def test_existing_request_parser_and_reply_policy_are_unchanged(self):
  path='rust/src/qcom_tftp_server_check.rs';current=(ROOT/path).read_text();old=previous(path)
  for name in ['fn missing_path(','fn receive_inner(','fn mcfg_request(']:self.assertEqual(function(current,name),function(old,name),name)
if __name__=='__main__':unittest.main()
