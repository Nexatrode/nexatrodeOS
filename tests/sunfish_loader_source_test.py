"""Private loader preparation pins board glue and the relocation stop point."""
import unittest
import os
import subprocess
import tempfile
from pathlib import Path
from tests.sunfish_modem_data_probe_test import load


class LoaderSourceTests(unittest.TestCase):
    def test_qrtr_credit_probe_uses_a_separate_exact_task_manifest(self):
        prepare = load('prepare-image-loader-sdk')
        chain = dict(handshake_modem_status=True, glink_open_probe=True,
                     glink_channel_probe=True, glink_intents_probe=True,
                     glink_rx_packet_probe=True, glink_done_probe=True,
                     qrtr_hello_probe=True, qrtr_next_probe=True,
                     qrtr_discovery_probe=True)
        discovery = prepare.task_manifest_path(**chain)
        credit = prepare.task_manifest_path(**chain, qrtr_credit_probe=True)
        self.assertEqual(discovery.name, 'loader-task-regions-qrtr-discovery.json')
        self.assertEqual(credit.name, 'loader-task-regions-qrtr-credit.json')
        self.assertNotEqual(discovery, credit)

    def test_qrtr_wait_probe_uses_its_own_exact_task_manifest(self):
        prepare = load('prepare-image-loader-sdk')
        chain = dict(handshake_modem_status=True, glink_open_probe=True,
                     glink_channel_probe=True, glink_intents_probe=True,
                     glink_rx_packet_probe=True, glink_done_probe=True,
                     qrtr_hello_probe=True, qrtr_next_probe=True,
                     qrtr_discovery_probe=True, qrtr_credit_probe=True)
        credit = prepare.task_manifest_path(**chain)
        wait = prepare.task_manifest_path(**chain, qrtr_wait_probe=True)
        self.assertEqual(wait.name, 'loader-task-regions-qrtr-wait.json')
        self.assertNotEqual(credit, wait)

    def test_servreg_restart_probe_uses_a_distinct_exact_task_manifest(self):
        prepare = load('prepare-image-loader-sdk')
        state = prepare.task_manifest_path(handshake_modem_status=True,
                                            modem_servreg_state_probe=True)
        restart = prepare.task_manifest_path(handshake_modem_status=True,
                                             modem_servreg_state_probe=True,
                                             modem_servreg_restart_probe=True)
        self.assertEqual(state.name, 'loader-task-regions-modem-servreg-state.json')
        self.assertEqual(restart.name, 'loader-task-regions-modem-servreg-restart.json')
        self.assertNotEqual(state, restart)

    def test_kernel_continuation_requires_task_loading_and_the_read_only_program(self):
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as temporary:
            out = Path(temporary) / 'must-not-build'
            result = subprocess.run(['python3', str(root / 'platforms/sunfish/prepare-image-loader-sdk.py'),
                                     '--out-dir', str(out), '--boot-diagnostic'], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('--boot-diagnostic requires --task-images', result.stderr)
            self.assertFalse(out.exists())
            env = dict(os.environ)
            env.update(SUNFISH_MODEM_IMAGE_BIN='rust_sunfish_modem_image_probe',
                       SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE='0', SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM='0',
                       SUNFISH_MODEM_IMAGE_OUT_DIR=str(out),
                       SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC='1',
                       SUNFISH_IMAGE_LOADER_TASK_IMAGES='0',
                       SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS='0',
                       SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION='0',
                       SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION='0',
                       SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY='0',
                       SUNFISH_IMAGE_LOADER_COPY_PROGRESS='0',
                       SUNFISH_IMAGE_LOADER_COPY_INPUTS='0')
            result = subprocess.run(['sh', str(root / 'platforms/sunfish/build-modem-image-probe.sh')],
                                    env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(out.exists())

            env.update(SUNFISH_IMAGE_LOADER_TASK_IMAGES='1',
                       SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS='1',
                       SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION='1',
                       SUNFISH_IMAGE_LOADER_CHECKPOINTS='1')
            result = subprocess.run(['sh', str(root / 'platforms/sunfish/build-modem-image-probe.sh')],
                                    env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(out.exists())

    def test_staging_requires_its_program_plan_and_verified_loader_path(self):
        root = Path(__file__).resolve().parents[1]
        prepare = load('prepare-image-loader-sdk')
        self.assertNotEqual(prepare.task_manifest_path(False), prepare.task_manifest_path(True))
        self.assertEqual(prepare.task_manifest_path(False,False,False,True,True).name,
                         'loader-task-regions-fatal-trace.json')
        with tempfile.TemporaryDirectory() as temporary:
            out = Path(temporary) / 'must-not-build'
            for flags in [['--stage-modem-image'], ['--stage-modem-image', '--boot-diagnostic']]:
                result = subprocess.run(['python3', str(root / 'platforms/sunfish/prepare-image-loader-sdk.py'),
                                         '--out-dir', str(out)] + flags, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(out.exists())
            env = dict(os.environ)
            env.update(SUNFISH_MODEM_IMAGE_BIN='rust_sunfish_modem_image_probe',
                       SUNFISH_MODEM_IMAGE_OUT_DIR=str(out), SUNFISH_IMAGE_LOADER_CHECKPOINTS='1',
                       SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE='1', SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC='0',
                       SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM='0',
                       SUNFISH_IMAGE_LOADER_TASK_IMAGES='1', SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS='1',
                       SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION='1', SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION='0',
                       SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY='0', SUNFISH_IMAGE_LOADER_COPY_PROGRESS='0',
                       SUNFISH_IMAGE_LOADER_COPY_INPUTS='0')
            for key, value in [('SUNFISH_MODEM_IMAGE_BIN', 'rust_sunfish_modem_image_boot_check'),
                               ('SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC', '1'),
                               ('SUNFISH_IMAGE_LOADER_TASK_IMAGES', '0'),
                               ('SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS', '0'),
                               ('SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION', '0'),
                               ('SUNFISH_IMAGE_LOADER_CHECKPOINTS', '0')]:
                changed = dict(env); changed[key] = value
                result = subprocess.run(['sh', str(root / 'platforms/sunfish/build-modem-image-probe.sh')],
                                        env=changed, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0, (key, value))
                self.assertFalse(out.exists())

    def test_auth_requires_its_separate_program_plan_and_complete_loader_gates(self):
        root = Path(__file__).resolve().parents[1]
        prepare = load('prepare-image-loader-sdk')
        self.assertNotEqual(prepare.task_manifest_path(True), prepare.task_manifest_path(False, True))
        with tempfile.TemporaryDirectory() as temporary:
            out = Path(temporary) / 'must-not-build'
            result = subprocess.run(['python3', str(root / 'platforms/sunfish/prepare-image-loader-sdk.py'),
                                     '--out-dir', str(out), '--authenticate-modem'], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0); self.assertFalse(out.exists())
            env = dict(os.environ)
            env.update(SUNFISH_MODEM_IMAGE_BIN='rust_sunfish_modem_auth_probe',
                       SUNFISH_MODEM_IMAGE_OUT_DIR=str(out), SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM='1',
                       SUNFISH_IMAGE_LOADER_CHECKPOINTS='1', SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC='0',
                       SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE='0', SUNFISH_IMAGE_LOADER_TASK_IMAGES='1',
                       SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS='1', SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION='1',
                       SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION='0', SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY='0',
                       SUNFISH_IMAGE_LOADER_COPY_PROGRESS='0', SUNFISH_IMAGE_LOADER_COPY_INPUTS='0')
            for key, value in [('SUNFISH_MODEM_IMAGE_BIN','rust_sunfish_modem_image_probe'),
                               ('SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM','0'),
                               ('SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC','1'),
                               ('SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE','1'),
                               ('SUNFISH_IMAGE_LOADER_TASK_IMAGES','0'),
                               ('SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS','0'),
                               ('SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION','0'),
                               ('SUNFISH_IMAGE_LOADER_CHECKPOINTS','0')]:
                changed=dict(env);changed[key]=value
                result=subprocess.run(['sh',str(root/'platforms/sunfish/build-modem-image-probe.sh')],env=changed,capture_output=True,text=True)
                self.assertNotEqual(result.returncode,0,(key,value));self.assertFalse(out.exists())

    def test_uart_is_idempotent_and_rejects_an_unexpected_board_branch(self):
        prepare = load('prepare-image-loader-sdk')
        text = '#if OTHER\n#elif defined(CONFIG_PLAT_QEMU_ARM_VIRT)\n#endif\n'
        patched = prepare.board_uart(text)
        self.assertEqual(prepare.board_uart(patched), patched)
        self.assertEqual(patched.count('#elif defined(CONFIG_PLAT_SUNFISH)'), 1)
        with self.assertRaises(AssertionError):
            prepare.board_uart(patched.replace('0x00a88000', '0xdeadbeef'))
        with self.assertRaises(AssertionError):
            prepare.board_uart('#if OTHER\n#endif\n')

    def test_relocation_hold_is_guarded_by_actual_runtime_address(self):
        prepare = load('prepare-image-loader-sdk')
        text = '    /* May not have been loaded in the right location. */\nrelocate:\n'
        ordinary = prepare.entry_checkpoints(text)
        self.assertNotIn('loader_relocated_hold', ordinary)
        held = prepare.entry_checkpoints(text, True)
        self.assertIn('cmp x1, x2\n    b.ne .Limage_continue_relocation', held)
        self.assertIn('loader_relocated_hold:\n    b loader_relocated_hold', held)
        self.assertLess(held.index('bl loader_checkpoint'), held.index('loader_relocated_hold:'))
        self.assertEqual(held.count('    .global loader_relocated_hold'), 1)
        with self.assertRaises(AssertionError):
            prepare.entry_checkpoints(held, True)

    def test_pre_relocation_hold_masks_interrupts_and_never_enters_relocation(self):
        prepare = load('prepare-image-loader-sdk')
        text = ('    mov     sp, x1\n'
                '    /* May not have been loaded in the right location. */\nrelocate:\n')
        held = prepare.entry_checkpoints(text, hold_before_relocation=True)
        self.assertIn('    mov     sp, x1\n    msr daifset, #0xf\n', held)
        self.assertIn('loader_entry_hold:\n    b loader_entry_hold\n', held)
        self.assertLess(held.index('bl loader_checkpoint'), held.index('loader_entry_hold:'))
        self.assertLess(held.index('loader_entry_hold:'), held.index('\nrelocate:'))
        self.assertNotIn('loader_relocated_hold', held)
        with self.assertRaises(AssertionError):
            prepare.entry_checkpoints(text, True, True)

    def test_copy_hold_verifies_before_cache_maintenance_and_skips_uart(self):
        prepare = load('prepare-image-loader-sdk')
        text = ('    mov     sp, x1\n'
                '    /* May not have been loaded in the right location. */\nrelocate:\n'
                '    bl relocation_log\n    bl memmove\n    bl flush_dcache\n')
        held = prepare.entry_checkpoints(text, hold_after_copy=True)
        self.assertIn('    msr daifset, #0xf\n', held)
        self.assertNotIn('    bl relocation_log', held)
        self.assertIn('loader_copy_already_placed:\n    b loader_copy_already_placed', held)
        self.assertIn('    b loader_copy_hold\n', held)
        self.assertLess(held.index('    bl memmove'), held.index('    b loader_copy_hold\n'))
        self.assertLess(held.index('    b loader_copy_hold\n'), held.index('    bl flush_dcache'))
        for before, after in [(True, False), (False, True)]:
            with self.assertRaises(AssertionError):
                prepare.entry_checkpoints(text, after, before, True)

    def test_progress_copy_replaces_the_byte_copy_and_always_holds_at_source(self):
        prepare = load('prepare-image-loader-sdk')
        text = ('    mov     sp, x1\n'
                '    /* May not have been loaded in the right location. */\nrelocate:\n'
                '    bl relocation_log\n    bl memmove\n    bl flush_dcache\n')
        held = prepare.entry_checkpoints(text, hold_after_copy=True, copy_progress=True)
        self.assertNotIn('    bl memmove', held)
        self.assertNotIn('    bl relocation_log', held)
        self.assertIn('    b loader_copy_progress_hold\n', held)
        self.assertLess(held.index('    b loader_copy_progress_hold\n'), held.index('    bl flush_dcache'))
        with self.assertRaises(AssertionError):
            prepare.entry_checkpoints(text, copy_progress=True)

    def test_input_snapshot_stops_before_copy_with_exclusive_mode(self):
        prepare = load('prepare-image-loader-sdk')
        text = ('    mov     sp, x1\n'
                '    /* May not have been loaded in the right location. */\nrelocate:\n'
                '    bl relocation_log\n    bl memmove\n    bl flush_dcache\n')
        held = prepare.entry_checkpoints(text, hold_after_copy=True, copy_inputs=True)
        self.assertNotIn('    bl memmove', held)
        self.assertNotIn('    bl relocation_log', held)
        self.assertIn('    b loader_copy_inputs_hold\n', held)
        self.assertLess(held.index('    b loader_copy_inputs_hold\n'), held.index('    bl flush_dcache'))
        with self.assertRaises(AssertionError):
            prepare.entry_checkpoints(text, copy_inputs=True)
        with self.assertRaises(AssertionError):
            prepare.entry_checkpoints(text, hold_after_copy=True, copy_inputs=True, copy_progress=True)

    def test_relocation_continues_only_after_verified_copy_and_keeps_native_interrupt_mask(self):
        prepare = load('prepare-image-loader-sdk')
        text = ('    mov     sp, x1\n'
                '    /* May not have been loaded in the right location. */\nrelocate:\n'
                '    bl relocation_log\n    bl memmove\n    mov x19, x0\n'
                '    bl flush_dcache\n    bl invalidate_icache\n    mov x0, x19\n')
        held = prepare.entry_checkpoints(text, hold_after_relocation=True, relocation_progress=True)
        self.assertIn('    msr daifset, #0xf\n', held)
        self.assertNotIn('    bl relocation_log', held)
        self.assertNotIn('    bl memmove', held)
        self.assertIn('    bl loader_copy_progress_resume\n', held)
        self.assertIn('loader_copy_hold_stop:\n    b loader_copy_hold_stop', held)
        self.assertLess(held.index('bl loader_copy_progress_resume'), held.index('bl flush_dcache'))
        self.assertLess(held.index('bl flush_dcache'), held.index('adr x0, .Limage_clean_text'))
        self.assertLess(held.index('bl invalidate_icache'), held.index('adr x0, .Limage_jump_text'))
        self.assertLess(held.index('adr x0, .Limage_jump_text'), held.index('mov x0, x19'))
        self.assertIn('loader_relocated_hold:\n    b loader_relocated_hold', held)
        with self.assertRaises(AssertionError):
            prepare.entry_checkpoints(text, relocation_progress=True)

    def test_task_loading_reaches_main_only_at_destination_and_has_a_separate_stop(self):
        prepare = load('prepare-image-loader-sdk')
        text = ('    mov     sp, x1\n'
                '    /* May not have been loaded in the right location. */\nrelocate:\n'
                '    bl relocation_log\n    bl memmove\n    mov x19, x0\n'
                '    bl flush_dcache\n    bl invalidate_icache\n    mov x0, x19\n')
        held = prepare.entry_checkpoints(text, hold_after_relocation=True, relocation_progress=True, task_images=True)
        self.assertIn('loader_relocated_hold:\n    b main', held)
        self.assertIn('loader_task_images_stop:\n    b loader_task_images_stop', held)
        self.assertIn('cmp x1, x2\n    b.ne .Limage_continue_relocation', held)
        with self.assertRaises(AssertionError):
            prepare.entry_checkpoints(text, task_images=True)


if __name__ == '__main__':
    unittest.main()
