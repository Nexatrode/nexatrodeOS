"""Follow-up adds no protocol, hardware or native runtime entry points."""
from pathlib import Path
import subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
class PredecessorBoundary(unittest.TestCase):
 def test_full_protocol_and_existing_observer_boundaries_unchanged(self):
  for name in ['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_elf_publication.rs','qcom_modem_owner.rs','qcom_mcfg_close.rs','qcom_owner_mcfg_close.rs','qcom_tftp_server_check.rs','qcom_qrtr_flow.rs','qcom_mcfg_ram.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs']:
   f='rust/src/'+name;self.assertEqual((ROOT/f).read_text(),subprocess.check_output(['git','show','39860731:'+f],cwd=ROOT,text=True),f)
 def test_predecessor_requires_full_original_reservation_identity(self):
  s=(ROOT/'rust/src/qcom_post_elf_mcfg.rs').read_text();self.assertIn('t.reference_flow==Some(key)',s);self.assertIn('t.kind.qrtr_addresses().is_some_and(|(_,to)|to==m.peer)',s);self.assertIn('m.admission.block_record==tx.reference_flow',s);self.assertIn('m.admission.block_record==self.transmit[index].reference_flow',s)
if __name__=='__main__':unittest.main()
