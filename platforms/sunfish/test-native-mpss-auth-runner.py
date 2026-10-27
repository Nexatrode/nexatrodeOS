#!/usr/bin/env python3
"""Exercise host boot gating/atomic consumption with synthetic files and USB.

No real fastboot/ADB executable or phone is used. This does not establish any
hardware authority, cold state, native MPSS start or Wi-Fi behavior.
"""
from pathlib import Path
import contextlib
import hashlib
import importlib.util
import io
import json
import os
import subprocess
import tempfile
import threading
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SOURCE = HERE / 'run-native-mpss-auth.py'
spec = importlib.util.spec_from_file_location('auth_runner', SOURCE)
r = importlib.util.module_from_spec(spec)
spec.loader.exec_module(r)
SOURCE_DIAGNOSTIC_FEATURE = 'sunfish-native-startup-mpss-auth-source-diagnostic'


class Fixture:
    def __init__(self, root):
        self.root = root
        self.out = root / 'experiment'
        self.out.mkdir(mode=0o700)
        self.source = root / 'src.rs'
        self.source.write_text('reviewed source\n')
        self.environment = root / 'kernel.elf'
        self.environment.write_bytes(b'modeled kernel')
        for folder in ('source', 'compiler-inputs', 'private-inputs'):
            (self.out / folder).mkdir()
        for folder in ('source', 'compiler-inputs'):
            (self.out / folder / 'src.rs').write_bytes(self.source.read_bytes())
        self.artifacts = {}
        for name in ['fastboot.img', 'capability-audit.json', 'boot-layout-audit.json',
                     'mpss-auth-source-audit.json', 'native_startup_preflight.elf',
                     'loader-sdk-profile.json', 'report.txt', 'capdl.json', 'cargo-build.log']:
            path = self.out / name
            if name == 'fastboot.img':
                path.write_bytes(b'ANDROID!' + bytes(32) + (2).to_bytes(4, 'little') + b'fixture')
            elif name in ('capability-audit.json', 'boot-layout-audit.json'):
                r.write(path, {'input_sha256': {str(self.source): r.identity(self.source)['sha256']}})
            else:
                path.write_text('fixture\n')
            self.artifacts[name] = r.identity(path)
        proof_source = root / 'proof.json'
        proof_source.write_text('{"result":"passed"}\n')
        proof_snapshot = self.out / 'proofs' / 'fixture' / 'proof.json'
        proof_snapshot.parent.mkdir(parents=True)
        proof_snapshot.write_bytes(proof_source.read_bytes())
        note_source = root / 'current-notes.md'
        note_source.write_text('notes\n')
        note_snapshot = self.out / 'notes' / 'current-notes.md'
        note_snapshot.parent.mkdir()
        note_snapshot.write_bytes(note_source.read_bytes())
        bound_source = root / 'bound-document.md'
        bound_source.write_text('proof-bound document\n')
        bound_snapshot = self.out / 'proof-bound-documents' / 'fixture' / 'bound-document.md'
        bound_snapshot.parent.mkdir(parents=True)
        bound_snapshot.write_bytes(bound_source.read_bytes())
        self.manifest = dict(feature=r.FEATURE, serial=r.SERIAL, product=r.PRODUCT,
            requires_full_poweroff_seconds_at_least=10, ready_for_cold_device_check=True,
            attempt_consumed=False, fastboot_attempted=False, device_actions=[], scope='host fixture only',
            diagnostic_only=False, source_guard_stop_before_relocation=False,
            artifacts=self.artifacts, source_sha256={'src.rs':r.identity(self.source)['sha256']},
            compiler_dependencies={'src.rs':r.identity(self.source)},
            environment_identities={str(self.environment):r.identity(self.environment)}, private_input_identities={},
            proofs={'fixture': {'path':str(proof_source), **r.identity(proof_source)}},
            proof_bound_documents={'fixture/docs/bound-document.md': {
                'path':'bound-document.md', 'snapshot':'proof-bound-documents/fixture/bound-document.md',
                'sha256':r.identity(bound_source)['sha256'], 'current_sha256':r.identity(bound_source)['sha256']}},
            current_notes={'path':str(note_source), 'snapshot':'notes/current-notes.md', **r.identity(note_source)})
        r.write(self.out / 'manifest.json', self.manifest)
        self.cold = root / 'cold.json'
        r.write(self.cold, dict(source='user_reply', user_text='Cold Fastboot ready',
            entered_fastboot_after_poweroff=True, poweroff_seconds_at_least=10,
            serial=r.SERIAL, image_sha256=self.artifacts['fastboot.img']['sha256'], recorded_utc=r.now()))
        self.commands = []
        self.values = {'product':r.PRODUCT,'unlocked':'yes','current-slot':'a','max-download-size':'0x10000000'}
        self.outcome = 0

    def invoke(self, args, timeout, log):
        self.commands.append(args)
        if args[1:] == ['devices', '-l']:
            answer = r.SERIAL + ' fastboot usb:fixture\n'
        elif 'getvar' in args:
            name = args[-1]
            answer = '(bootloader) ' + name + ': ' + self.values[name] + '\nFinished. Total time: 0.001s\n'
        else:
            assert args[1:4] == ['-s', r.SERIAL, 'boot']
            record = r.read(self.out / 'attempt.json')
            assert record['status'] == 'consumed_before_fastboot_boot'
            assert record['automatic_retry_allowed'] is False
            assert Path(args[-1]).parent.name == 'artifacts'
            assert r.identity(Path(args[-1])) == self.artifacts['fastboot.img']
            if isinstance(self.outcome, BaseException):
                raise self.outcome
            log.write_text('modeled boot command\n')
            return self.outcome, 'modeled boot command\n'
        log.write_text(answer)
        return 0, answer

    def execute(self):
        with contextlib.redirect_stdout(io.StringIO()):
            return r.execute(self.out, self.cold, self.root / 'runs', '/modeled/fastboot', self.root, self.invoke)

    def boots(self):
        return [c for c in self.commands if 'boot' in c]


class Cases(unittest.TestCase):
    def fixture(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        return Fixture(Path(temporary.name))

    def source_diagnostic(self, fixture):
        fixture.manifest['feature'] = SOURCE_DIAGNOSTIC_FEATURE
        fixture.manifest['diagnostic_only'] = True
        fixture.manifest['source_guard_stop_before_relocation'] = True
        fixture.manifest['scope'] = 'source guard diagnostic only; stops before relocation'
        r.write(fixture.out / 'loader-sdk-profile.json', {'native_source_diagnostic': True})
        r.write(fixture.out / 'boot-layout-audit.json', {
            'input_sha256': {str(fixture.source): r.identity(fixture.source)['sha256']},
            'source_guard_diagnostic_audit': {
                'all_paths_stop_before_relocation': True, 'diagnostic_calls': 6}})
        fixture.artifacts['loader-sdk-profile.json'] = r.identity(fixture.out / 'loader-sdk-profile.json')
        fixture.artifacts['boot-layout-audit.json'] = r.identity(fixture.out / 'boot-layout-audit.json')
        fixture.manifest['artifacts'] = fixture.artifacts
        r.write(fixture.out / 'manifest.json', fixture.manifest)
        return fixture

    def test_offline_verify_does_not_touch_usb_or_claim(self):
        f = self.fixture()
        r.verify(f.out, f.root)
        self.assertEqual(f.commands, [])
        self.assertFalse((f.out / 'attempt.json').exists())

    def test_changed_image_source_snapshot_environment_or_feature_stops_before_boot(self):
        for kind in ('image','source','snapshot','environment','feature'):
            with self.subTest(kind=kind):
                f = self.fixture()
                if kind == 'image': (f.out / 'fastboot.img').write_bytes(b'changed')
                if kind == 'source': f.source.write_text('changed')
                if kind == 'snapshot': (f.out / 'source/src.rs').write_text('changed')
                if kind == 'environment': f.environment.write_bytes(b'changed')
                if kind == 'feature':
                    f.manifest['feature'] = 'different'
                    r.write(f.out / 'manifest.json', f.manifest)
                with self.assertRaises(ValueError): f.execute()
                self.assertEqual(f.commands, [])
                self.assertFalse((f.out / 'attempt.json').exists())

    def test_missing_or_wrong_cold_confirmation_never_sends_boot(self):
        for field, value in [('source','agent_inference'),('entered_fastboot_after_poweroff',False),
                             ('poweroff_seconds_at_least',9),('serial','other'),('image_sha256','other')]:
            with self.subTest(field=field):
                f = self.fixture()
                c = r.read(f.cold); c[field] = value; r.write(f.cold, c)
                with self.assertRaises(ValueError): f.execute()
                self.assertEqual(f.commands, [])
                self.assertFalse((f.out / 'attempt.json').exists())

    def test_wrong_identity_or_download_limit_never_claims_or_boots(self):
        for field, value in [('product','other'),('unlocked','no'),('current-slot','b'),('max-download-size','0x1000')]:
            with self.subTest(field=field):
                f = self.fixture(); f.values[field] = value
                with self.assertRaises(ValueError): f.execute()
                self.assertEqual(f.boots(), [])
                self.assertFalse((f.out / 'attempt.json').exists())

    def test_consumed_before_one_boot_and_second_execution_never_replays(self):
        f = self.fixture()
        code, run = f.execute()
        self.assertEqual(code,0)
        self.assertEqual(len(f.boots()),1)
        self.assertEqual(r.read(f.out / 'attempt.json')['status'],'fastboot_command_completed')
        self.assertFalse(r.read(run / 'attempt.json')['native_mpss_started'])
        self.assertTrue((run / 'artifacts/source/src.rs').is_file())
        with self.assertRaises(ValueError): f.execute()
        self.assertEqual(len(f.boots()),1)

    def test_failure_or_timeout_consumes_without_reset_or_retry(self):
        for outcome in (1,subprocess.TimeoutExpired(['modeled-fastboot'],120)):
            with self.subTest(outcome=type(outcome).__name__):
                f = self.fixture(); f.outcome = outcome
                if isinstance(outcome, BaseException):
                    with self.assertRaises(subprocess.TimeoutExpired): f.execute()
                else: self.assertEqual(f.execute()[0],1)
                record = r.read(f.out / 'attempt.json')
                self.assertIn(record['status'],('fastboot_command_failed','fastboot_outcome_uncertain'))
                self.assertFalse(record['automatic_retry_allowed'])
                with self.assertRaises(ValueError): f.execute()
                self.assertEqual(len(f.boots()),1)
                self.assertFalse(any('reboot' in c or 'flash' in c for c in f.commands))

    def test_atomic_claim_has_one_winner(self):
        f = self.fixture(); barrier = threading.Barrier(2); wins = []; losses = []
        path = f.out / 'attempt.json'
        def claim(n):
            barrier.wait()
            try: r.write(path, {'owner':n}, exclusive=True); wins.append(n)
            except FileExistsError: losses.append(n)
        threads = [threading.Thread(target=claim,args=(n,))for n in range(2)]
        for t in threads:t.start()
        for t in threads:t.join()
        self.assertEqual(len(wins),1)
        self.assertEqual(len(losses),1)
        self.assertEqual(r.read(path)['owner'],wins[0])

    def test_source_diagnostic_feature_is_accepted_for_verified_image(self):
        f = self.source_diagnostic(self.fixture())
        manifest = r.verify(f.out, f.root)
        self.assertEqual(manifest['feature'], SOURCE_DIAGNOSTIC_FEATURE)
        self.assertTrue(manifest['diagnostic_only'])

    def test_source_diagnostic_requires_audited_pre_relocation_stop(self):
        f = self.source_diagnostic(self.fixture())
        layout = r.read(f.out / 'boot-layout-audit.json')
        layout['source_guard_diagnostic_audit']['all_paths_stop_before_relocation'] = False
        r.write(f.out / 'boot-layout-audit.json', layout)
        f.manifest['artifacts']['boot-layout-audit.json'] = r.identity(f.out / 'boot-layout-audit.json')
        r.write(f.out / 'manifest.json', f.manifest)
        with self.assertRaises(ValueError):
            r.verify(f.out, f.root)

    def test_changed_frozen_evidence_never_claims_or_boots(self):
        for item in ('proof', 'proof-bound-document', 'current-notes'):
            with self.subTest(item=item):
                f = self.fixture()
                if item == 'proof':
                    (f.out / 'proofs/fixture/proof.json').write_text('altered proof\n')
                elif item == 'proof-bound-document':
                    (f.out / 'proof-bound-documents/fixture/bound-document.md').write_text('altered doc\n')
                else:
                    (f.out / 'notes/current-notes.md').write_text('altered notes\n')
                with self.assertRaises(ValueError):
                    f.execute()
                self.assertEqual(f.commands, [])
                self.assertFalse((f.out / 'attempt.json').exists())

    def test_auth_manifest_is_not_misclassified_as_source_diagnostic(self):
        for feature, diagnostic_only in ((r.FEATURE, True), (SOURCE_DIAGNOSTIC_FEATURE, False)):
            with self.subTest(feature=feature, diagnostic_only=diagnostic_only):
                f = self.fixture()
                f.manifest['feature'] = feature
                f.manifest['diagnostic_only'] = diagnostic_only
                r.write(f.out / 'manifest.json', f.manifest)
                with self.assertRaises(ValueError):
                    r.verify(f.out, f.root)

    def test_cold_confirmation_binds_source_diagnostic_image_hash(self):
        f = self.source_diagnostic(self.fixture())
        cold = r.read(f.cold)
        cold['image_sha256'] = 'wrong-image-hash'
        r.write(f.cold, cold)
        with self.assertRaises(ValueError):
            f.execute()
        self.assertEqual(f.commands, [])
        self.assertFalse((f.out / 'attempt.json').exists())

    def test_consumed_source_diagnostic_cannot_be_retried(self):
        f = self.source_diagnostic(self.fixture())
        self.assertEqual(f.execute()[0], 0)
        with self.assertRaises(ValueError):
            f.execute()
        self.assertEqual(len(f.boots()), 1)
        record = r.read(f.out / 'attempt.json')
        self.assertFalse(record['automatic_retry_allowed'])


if __name__ == '__main__':
    os.umask(0o077)
    out = ROOT / '.cache/sunfish-native-wifi/private/mpss-auth-runner'
    out.mkdir(parents=True,exist_ok=True,mode=0o700)
    hashes = {str(p.relative_to(ROOT)): r.identity(p)['sha256'] for p in (SOURCE,Path(__file__))}
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(Cases)
    with (out / 'tests.log').open('w') as log:
        result = unittest.TextTestRunner(stream=log,verbosity=2).run(suite)
    if not result.wasSuccessful():raise SystemExit('Host runner tests failed: ' + str(out / 'tests.log'))
    for name, expected in hashes.items():
        if r.identity(ROOT / name)['sha256'] != expected:raise ValueError('runner source changed during tests')
    r.write(out / 'result.json',dict(result='passed',source_sha256=hashes,groups=result.testsRun,
        scope='Actual host verifier/archive/atomic-consumption code; synthetic files and modeled fastboot responses only.',
        device_actions=[],physical_cold_state_proven=False,native_mpss_started=False,native_wifi_proven=False))
    print(f'PASS: {result.testsRun} host boot-gating groups; no phone or fastboot executable used.')
