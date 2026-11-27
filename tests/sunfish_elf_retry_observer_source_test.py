"""Remove only the bounded observer/renderer to recover prior protocol source."""
from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='0e7fca41'
COUNTS={'rust/src/qcom_modem_owner.rs':8,'rust/src/sunfish_ipa_linux_qmi_owner.rs':1,'rust/src/pixel_mpss_elf_prequeue_owners.rs':2}
def without_retry(path,source):
 for name in ['ELF_RETRY_OBSERVER','TX3_RETRY_PANEL']:
  pat=r'/\* '+name+r'_BEGIN \*/\n.*?/\* '+name+r'_END \*/\n';blocks=re.findall(pat,source,re.S)
  expected=COUNTS.get(path,0)if (name=='ELF_RETRY_OBSERVER' and path in ['rust/src/qcom_modem_owner.rs','rust/src/sunfish_ipa_linux_qmi_owner.rs']) or (name=='TX3_RETRY_PANEL' and path=='rust/src/pixel_mpss_elf_prequeue_owners.rs') else 0
  assert len(blocks)==expected,(path,name,len(blocks),expected)
  assert all('#[cfg(feature="sunfish-tx3-elf-retry")]'in b for b in blocks)
  source=re.sub(pat,'',source,flags=re.S)
 if path=='rust/Cargo.toml':
  source=source.replace('sunfish-startup-pressure = ["sunfish-elf-pool-release"]\n','')
  source=source.replace('sunfish-elf-pool-release = ["sunfish-tx3-elf-retry"]\n','')
  source=source.replace('sunfish-tx3-elf-retry = ["sunfish-elf-prequeue-owners"]\n','')
 return source
UNCHANGED=['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_elf_window.rs','qcom_elf_slot_turn.rs','qcom_elf_capacity_turn.rs','qcom_elf_publication.rs','qcom_elf_tx_slots.rs','qcom_qrtr_flow.rs','qcom_efs_overlap.rs','qcom_owner_efs_overlap.rs','qcom_fifo_efs_reply.rs','qcom_owner_efs_service.rs','qcom_owner_efs_early.rs','qcom_fifo_efs_early.rs','qcom_tftp_server_check.rs','qcom_owner_mcfg_close.rs','pixel_mpss_runtime.rs','pixel_mpss_runtime_irq.rs','pixel_mpss_post_elf_outcome.rs','pixel_mpss_post_efs_elf.rs','sunfish_ipa_linux_qmi_owner.rs','sunfish_smmu_native.rs']
class Boundary(unittest.TestCase):
 def test_execution_source_recovers_exact_prior(self):
  for n in UNCHANGED+['qcom_modem_owner.rs','pixel_mpss_elf_prequeue_owners.rs']:
   f='rust/src/'+n;self.assertEqual(without_retry(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
  self.assertEqual(without_retry('rust/Cargo.toml',(ROOT/'rust/Cargo.toml').read_text()),subprocess.check_output(['git','show',BASE+':rust/Cargo.toml'],text=True))
 def test_trace_cannot_authorize_io(self):
  s=(ROOT/'rust/src/qcom_owner_elf_retry.rs').read_text()
  for bad in ['unsafe','with_io','revalidate','queue_reply','publish_next','interrupts','self.adapter.take_','self.adapter.ingest','self.elf_slot_turn_taken=','self.scheduled=']:self.assertNotIn(bad,s)
  for expected in ['b.handle==completed','self.requests.contains','self.elf_retry.flags&2','self.elf_retry.flags&4','self.elf_retry.flags&8','self.elf_retry.flags&16','self.elf_retry.flags&32','self.elf_retry.receipt_at=b.done']:self.assertIn(expected,s)
  self.assertIn('let stopped=self.stopped;self.stopped=true;',s);self.assertIn('self.stopped=stopped;Some(at)',s)
  p=(ROOT/'rust/src/pixel_mpss_tx3_elf_retry.rs').read_text()
  for bad in ['&mut','unsafe','with_io','publish_next','revalidate']:self.assertNotIn(bad,p)
if __name__=='__main__':unittest.main()
