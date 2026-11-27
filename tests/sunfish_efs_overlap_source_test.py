from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1]
PATTERN=r'/\* EFS_REPLY_OVERLAP_BEGIN \*/\n.*?/\* EFS_REPLY_OVERLAP_END \*/\n'
COUNTS={'rust/src/qcom_glink_qmi.rs':8,'rust/src/qcom_glink_fifo.rs':1,'rust/src/qcom_modem_owner.rs':1,'rust/src/pixel_mpss_runtime.rs':1,'rust/src/qcom_fifo_efs_reply.rs':1}
def without_overlap(path,source):
 blocks=re.findall(PATTERN,source,re.S)
 if path in COUNTS:assert len(blocks)==COUNTS[path],(path,len(blocks));assert all('#[cfg(feature="sunfish-efs-reply-overlap")]'in b for b in blocks)
 else:assert not blocks,path
 return re.sub(PATTERN,'',source,flags=re.S).replace('reads[efs_reply::watched_index(&self.session)]','reads[2]')
class Boundary(unittest.TestCase):
 def test_disabled_source_and_unchanged_checks(self):
  for n in ['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_modem_owner.rs','pixel_mpss_runtime.rs','qcom_qrtr_flow.rs','qcom_owner_efs_service.rs','qcom_fifo_efs_reply.rs','qcom_owner_efs_early.rs','qcom_fifo_efs_early.rs','qcom_startup_prerequisites.rs','qcom_tftp_server_check.rs','qcom_owner_mcfg_close.rs','pixel_mpss_runtime_irq.rs','sunfish_ipa_linux_qmi_owner.rs']:
   f='rust/src/'+n;self.assertEqual(without_overlap(f,(ROOT/f).read_text()),subprocess.check_output(['git','show','3c7e3ef9:'+f],text=True),f)
 def test_actual_pair_bytes_and_original_live_transmit(self):
  s=(ROOT/'rust/src/qcom_efs_overlap.rs').read_text();core=(ROOT/'rust/src/qcom_glink_qmi.rs').read_text()
  for part in ['REQUEST_LIMIT:usize=256','body==&p.bytes[..p.len]','p.second!=Some(*handle)','p.closed','p.second_queued','tx.sent!=tx.len','tx.glink_done','tx.remote_slot.is_none()','tx.reference_flow!=Some(token)','is_transfer_port(route.source.port)','p.first_tx.is_none()']:self.assertIn(part,s)
  for name in ['startup_prerequisites','intent_ack_clock','finish_after_glink','publish_next','unsafe','read_volatile']:self.assertNotIn(name,s)
  self.assertIn('self.efs_overlap=efs_overlap::EfsOverlap::EMPTY',core);self.assertIn('self.bind_efs_overlap_queue(handle,route,packet,result,overlap)',core)
if __name__=='__main__':unittest.main()
