import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('handoff_stack', Path(__file__).resolve().parents[1] / 'platforms/sunfish/audit-runtime-handoff-stack.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class FrameParserTests(unittest.TestCase):
    def test_page_probe_loop_counted_once_with_tail_and_saves(self):
        text = ''' 1000: stp x29, x30, [sp, #-0x60]!
1004: sub x9, sp, #0x190, lsl #12
1008: sub sp, sp, #0x1, lsl #12
100c: ldr xzr, [sp]
1010: cmp sp, x9
1014: b.ne 0x1008
1018: sub sp, sp, #0x590
101c: bl 0x2000'''
        self.assertEqual(audit.frame_from_disassembly(text), 0x1905f0)

    def test_direct_small_reserve_and_preindexed_save(self):
        self.assertEqual(audit.frame_from_disassembly('1000: str x29, [sp, #-0x50]!\n1004: sub sp, sp, #0x80\n1008: bl 0x2000'), 0xd0)

    def test_incomplete_probe_is_not_silently_accepted(self):
        with self.assertRaises(ValueError):
            audit.frame_from_disassembly('1000: sub x9, sp, #0x100, lsl #12\n1004: bl 0x2000')

    def test_second_function_after_call_not_added(self):
        self.assertEqual(audit.frame_from_disassembly('1000: sub sp, sp, #0x80\n1004: bl 0x2000\n1008: sub sp, sp, #0x100'), 0x80)

    def test_dynamic_stack_write_is_rejected(self):
        with self.assertRaises(ValueError):
            audit.frame_from_disassembly('1000: sub sp, sp, x9\n1004: bl 0x2000')

    def test_truncated_or_unrecognized_prefix_is_rejected(self):
        with self.assertRaises(ValueError):
            audit.frame_from_disassembly('1000: nop')

    def test_probe_requires_compare_and_exact_backedge(self):
        with self.assertRaises(ValueError):
            audit.frame_from_disassembly('1000: sub x9, sp, #0x100, lsl #12\n1004: sub sp, sp, #0x1, lsl #12\n1008: bl 0x2000')

    def test_zero_probe_target_is_not_a_zero_frame(self):
        with self.assertRaises(ValueError):
            audit.frame_from_disassembly('1000: sub x9, sp, #0x0, lsl #12\n1004: sub sp, sp, #0x1, lsl #12\n1008: ldr xzr, [sp]\n100c: cmp sp, x9\n1010: b.ne 0x1004\n1014: bl 0x2000')

    def test_init_selection_accepts_only_exact_function_or_inlined_call_once(self):
        direct='rust_sunfish_native_startup_preflight::init'
        wrapper='<rust_sunfish_native_startup_preflight::init as core::ops::function::FnOnce<()>>::call_once'
        self.assertEqual(audit.choose_init_symbol([wrapper,direct]),(direct,wrapper))
        self.assertEqual(audit.choose_init_symbol([wrapper]),(wrapper,None))
        self.assertEqual(audit.choose_init_symbol([direct]),(direct,None))
        self.assertEqual(audit.choose_init_symbol([wrapper+' (.llvm.123)']), (wrapper+' (.llvm.123)',None))
        for names in [[direct+'::{closure#3}'],[wrapper,wrapper+' (.llvm.123)'],[]]:
            with self.assertRaises(ValueError):audit.choose_init_symbol(names)

    def test_outlined_init_thunk_requires_exact_zero_stack_forwarder(self):
        text='1000: mov x8, x0\n1004: b 0x2000 <init>'
        self.assertEqual(audit.zero_frame_init_thunk(text,0x2000),0)
        for other in [text.replace('2000','3000'),'1000: sub sp, sp, #0x10\n'+text,text.replace('mov x8, x0','mov sp, x0')]:
            with self.assertRaises(ValueError):audit.zero_frame_init_thunk(other,0x2000)


if __name__ == '__main__':
    unittest.main()
