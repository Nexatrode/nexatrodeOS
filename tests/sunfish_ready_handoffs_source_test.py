from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='8f10d267'
def without_ready(path,source):
 pat=r'/\* READY_HANDOFF_BEGIN \*/\n.*?/\* READY_HANDOFF_END \*/\n'
 source=re.sub(pat,'',source,flags=re.S)
 if path=='rust/src/qcom_glink_qmi.rs':source=source.replace('                }\n                { 3 }','                } { 3 }')
 if path=='rust/Cargo.toml':source=source.replace('sunfish-registry-ready-handoffs = ["sunfish-startup-pressure"]\n','')
 return source
class Boundary(unittest.TestCase):
 def test_disabled_delta_recovers_every_parent_rust_source(self):
  files=subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']
  for f in files:self.assertEqual(without_ready(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_authority_is_live_state_not_diagnostics(self):
  for f in ['qcom_registry_ready_handoff.rs','qcom_owner_ready_handoff.rs']:
   s=(ROOT/'rust/src'/f).read_text()
   for bad in ['elf_window_trace','service_registry_trace','rmtfs_event_clock','intent_ack_clock','unsafe','with_io']:self.assertNotIn(bad,s)
  s=(ROOT/'rust/src/qcom_owner_ready_handoff.rs').read_text();self.assertIn('self.adapter.take_elf_slot_turn(&completed,&waiting)',s);self.assertIn('self.dispatch_service_registry_step(interrupts)',s);self.assertIn('self.elf_ready_handoff.ready.take()',s)
 def test_runtime_attempt_precedes_other_service_work(self):
  s=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text();a=s.index('    fn service_pass(');z=s.index('        let server = (&mut *self.boot).server().map_err(|_| 0x51u32)?;',a);call='self.router.service_ready_blocker(owner)'
  self.assertEqual(s.count(call),1);self.assertIn(call,s[a:z]);self.assertIn('self.router.dispatch_queued_elf_reply(owner)',s[a:z])
if __name__=='__main__':unittest.main()
