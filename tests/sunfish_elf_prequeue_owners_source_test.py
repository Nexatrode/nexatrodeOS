"""The prequeue screen changes presentation, not the retained admission machinery."""
from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
BASE='737501ff'
PATTERN=r'/\* ELF_PREQUEUE_PANEL_BEGIN \*/\n.*?/\* ELF_PREQUEUE_PANEL_END \*/\n'
COUNTS={'rust/src/pixel_mpss_post_efs_elf.rs':2}
def without_panel(path,source):
 from sunfish_elf_retry_observer_source_test import without_retry
 source=without_retry(path,source)
 blocks=re.findall(PATTERN,source,re.S)
 assert len(blocks)==COUNTS.get(path,0),(path,len(blocks))
 assert all('#[cfg(feature="sunfish-elf-prequeue-owners")]'in b for b in blocks)
 return re.sub(PATTERN,'',source,flags=re.S)
UNCHANGED=['qcom_elf_window.rs','qcom_elf_tx_slots.rs','qcom_elf_publication.rs','qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_modem_owner.rs','qcom_qrtr_flow.rs','qcom_efs_overlap.rs','qcom_owner_efs_overlap.rs','qcom_fifo_efs_overlap.rs','qcom_fifo_efs_reply.rs','qcom_owner_efs_service.rs','qcom_owner_efs_early.rs','qcom_fifo_efs_early.rs','qcom_startup_prerequisites.rs','qcom_tftp_server_check.rs','qcom_owner_mcfg_close.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','pixel_mpss_post_elf_outcome.rs','sunfish_ipa_linux_qmi_owner.rs','qcom_mcfg_ram.rs','sunfish_smmu_native.rs']
class Boundary(unittest.TestCase):
 def test_existing_execution_and_producers_are_identical(self):
  for n in UNCHANGED+['pixel_mpss_post_efs_elf.rs']:
   f='rust/src/'+n;self.assertEqual(without_panel(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
  line='sunfish-elf-prequeue-owners = ["sunfish-post-efs-elf-writer"]\n';s=(ROOT/'rust/Cargo.toml').read_text();from sunfish_elf_retry_observer_source_test import without_retry;s=without_retry('rust/Cargo.toml',s);self.assertEqual(s.count(line),1);self.assertEqual(s.replace(line,''),subprocess.check_output(['git','show',BASE+':rust/Cargo.toml'],text=True))
 def test_only_read_only_existing_metadata(self):
  s=(ROOT/'rust/src/pixel_mpss_elf_prequeue_owners.rs').read_text();self.assertIn('fn show_elf_prequeue_owners(&self,d:&Display,owner:&ModemOwner)',s)
  for bad in ['&mut','unsafe','read_volatile','with_io','revalidate','publish_next','queue_response','finish_after_glink','clock.ticks','owner.sample','owner.receive','owner.transmit']:self.assertNotIn(bad,s)
  for part in ['w.receipts[i*2]','w.first_failure','w.last_failure','c.first_free','c.latest.slots[i]','f.original','e.dispatch.selected','e.waits_by_reason','t.runs','"ELF PREQUEUE OWNERS"']:self.assertIn(part,s)
if __name__=='__main__':unittest.main()
