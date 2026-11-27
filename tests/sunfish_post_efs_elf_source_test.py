"""The post-EFS screen reuses retained data without altering protocol execution."""
from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
BASE='5e32c158'
PATTERN=r'/\* POST_EFS_ELF_PANEL_BEGIN \*/\n.*?/\* POST_EFS_ELF_PANEL_END \*/\n'
COUNTS={'rust/src/pixel_mpss_post_elf_outcome.rs':2}
def without_panel(path,source):
 from sunfish_elf_retry_observer_source_test import without_retry
 source=without_retry(path,source)
 blocks=re.findall(PATTERN,source,re.S)
 assert len(blocks)==COUNTS.get(path,0),(path,len(blocks))
 assert all('#[cfg(feature="sunfish-post-efs-elf-writer")]'in b for b in blocks)
 return re.sub(PATTERN,'',source,flags=re.S)
UNCHANGED=['qcom_elf_publication.rs','qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_modem_owner.rs','qcom_qrtr_flow.rs','qcom_efs_overlap.rs','qcom_owner_efs_overlap.rs','qcom_fifo_efs_overlap.rs','qcom_fifo_efs_reply.rs','qcom_owner_efs_service.rs','qcom_owner_efs_early.rs','qcom_fifo_efs_early.rs','qcom_startup_prerequisites.rs','qcom_tftp_server_check.rs','qcom_owner_mcfg_close.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','sunfish_ipa_linux_qmi_owner.rs','qcom_mcfg_ram.rs','sunfish_smmu_native.rs']
class Boundary(unittest.TestCase):
 def test_existing_execution_and_producer_are_identical(self):
  for n in UNCHANGED+['pixel_mpss_post_elf_outcome.rs']:
   f='rust/src/'+n
   self.assertEqual(without_panel(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_renderer_only_reads_retained_records(self):
  s=(ROOT/'rust/src/pixel_mpss_post_efs_elf.rs').read_text()
  self.assertIn('fn show_post_efs_elf(&self,d:&Display,owner:&ModemOwner)',s)
  for forbidden in ['&mut','unsafe','read_volatile','with_io','revalidate','publish_next','queue_response','finish_after_glink','clock.ticks','owner.sample','owner.receive','owner.transmit']:
   self.assertNotIn(forbidden,s)
  for part in ['w.records[i]','r.result','p.turn_gates','p.grant_gates','w.active','r.pending_message_id','"POST EFS ELF WRITER"']:
   self.assertIn(part,s)
  self.assertLess((ROOT/'rust/src/pixel_mpss_post_elf_outcome.rs').read_text().index('self.show_post_efs_elf'),(ROOT/'rust/src/pixel_mpss_post_elf_outcome.rs').read_text().index('self.show_startup_prerequisites'))
if __name__=='__main__':unittest.main()
