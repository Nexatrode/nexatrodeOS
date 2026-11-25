"""Bind the native service boundary to the actual Router/Owner replayed by host tests."""
from pathlib import Path
import subprocess, unittest
from sunfish_mcfg_trace_source_test import protocol_source
ROOT=Path(__file__).resolve().parents[1]
def fn(s,name):
 a=s.index('fn '+name);b=s.index('{',a)+1;depth=1
 while depth:depth+=(s[b]=='{')-(s[b]=='}');b+=1
 return s[a:b]
class HandoffSource(unittest.TestCase):
 def test_native_uses_the_tested_boundary_before_rmtfs(self):
  s=fn((ROOT/'rust/src/pixel_mpss_runtime.rs').read_text(),'service_pass(')
  a=s.index('registry_result.map_err');b=s.index('owner.dispatch_linux_rmtfs_burst',a);window=s[a:b]
  self.assertIn('#[cfg(feature="sunfish-mcfg-close-handoff")]',window)
  self.assertIn('self.router.dispatch_queued_elf_mcfg_handoff(owner,&mut self.server_check)',window)
  self.assertIn('not(feature="sunfish-mcfg-close-handoff")',window)
 def test_original_transport_and_cleanup_commit_are_unchanged(self):
  for name in ['qcom_glink_qmi.rs','qcom_glink_fifo.rs','qcom_mcfg_close.rs','qcom_qrtr_flow.rs']:
   path='rust/src/'+name
   self.assertEqual(protocol_source(path,(ROOT/path).read_text()),subprocess.check_output(['git','show','bb9f990c:'+path],cwd=ROOT,text=True),path)
 def test_registry_retry_already_defers_pending_exclusive_closes(self):
  s=(ROOT/'rust/src/qcom_modem_owner.rs').read_text();retry=fn(s,'retry_registry_handoff(')
  self.assertLess(retry.index('self.firmware_close_is_pending()'),retry.index('self.check()?'))
  self.assertIn('self.pending_mcfg_close.is_some()',fn(s,'firmware_close_is_pending('))
 def test_handoff_does_not_add_ordinary_io_when_no_close_pending(self):
  s=fn((ROOT/'rust/src/pixel_mpss_runtime_irq.rs').read_text(),'dispatch_queued_elf_mcfg_handoff')
  self.assertLess(s.index('self.live()'),s.index('owner.resolve_mcfg_close'))
  self.assertLess(s.index('owner.generation()!=self.generation'),s.index('owner.resolve_mcfg_close'))
  self.assertLess(s.index('self.other!=0'),s.index('owner.resolve_mcfg_close'))
  self.assertNotIn('.publish',s);self.assertNotIn('.revalidate',s)
if __name__=='__main__':unittest.main()
