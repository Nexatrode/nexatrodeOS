"""Exercise actual presentation gate and runtime busy predicate, with integration checks."""
from pathlib import Path
import json,subprocess,tempfile,unittest
ROOT=Path(__file__).resolve().parents[1]
class RegistryPaint(unittest.TestCase):
 def test_actual_gate_and_runtime_integration(self):
  source=(ROOT/'rust/src/pixel_mpss_runtime.rs').read_text();a=source.index('    fn registry_display_busy(');end=source.index('{',a)+1;depth=1
  while depth:depth+=(source[end]=='{')-(source[end]=='}');end+=1
  body=source[a:end]
  code='''#[path=GATE]mod gate;
struct Report{continuation_pending:bool}struct Router{busy:bool}impl Router{fn report(&self)->Report{Report{continuation_pending:self.busy}}}
enum Stage{Services(()),Opening(())}struct RuntimeDomain{stage:Option<Stage>,router:Router}
impl RuntimeDomain{BODY}
#[test]fn runtime_predicate_uses_live_continuation_and_service_stage(){let mut r=RuntimeDomain{stage:None,router:Router{busy:true}};assert!(!r.registry_display_busy());r.stage=Some(Stage::Opening(()));assert!(!r.registry_display_busy());r.stage=Some(Stage::Services(()));assert!(r.registry_display_busy());r.router.busy=false;assert!(!r.registry_display_busy());}
'''.replace('GATE',json.dumps(str(ROOT/'rust/src/pixel_mpss_registry_paint.rs'))).replace('BODY',body)
  with tempfile.TemporaryDirectory() as tmp:
   p=Path(tmp);(p/'test.rs').write_text(code);subprocess.run(['rustc','+nightly-2026-09-25','--edition=2024','--test','-Adead_code',str(p/'test.rs'),'-o',str(p/'test')],check=True);subprocess.run([str(p/'test'),'--test-threads=1'],check=True)
  progress=source[source.index('    fn paint_progress('):source.index('    fn timed_show(')]
  self.assertLess(progress.index('registry_paint.progress'),progress.index('display.runtime_progress'))
  self.assertLess(progress.index('progress_trace.checkpoint'),progress.index('registry_paint.progress'))
  self.assertLess(progress.index('awaiting_first_oack_completion'),progress.index('registry_paint.progress'))
  notified=source[source.index('    fn notified('):source.index('    fn fail(')]
  self.assertIn('self.registry_paint.full(self.error,busy,repaint)',notified)
  self.assertLess(notified.index('registry_paint.full'),notified.index('if repaint { self.timed_show(display); }'))
  for name in ['paint_progress','timed_show']:
   a=source.index('    fn '+name+'(');b=source.index('\n    }',a);self.assertIn('self.record_registry_paint(started,ended)',source[a:b])
if __name__=='__main__':unittest.main()
