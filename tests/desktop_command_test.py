"""Check the entry point's real data guards before any UTM mutation occurs."""
import importlib.util
import json
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('desktop_command',Path(__file__).resolve().parents[1]/'tools/desktop.py')
desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)


class CommandGuards(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name).resolve();self.guest=self.root/'build/desktop-apps/test'
        self.guest.mkdir(parents=True);self.bundle=self.guest/'Desktop-Apps-TEST.utm';self.data=self.bundle/'Data';self.data.mkdir(parents=True)
        self.addCleanup(patch.stopall)
        patch.object(desktop,'ROOT',self.root).start()
        patch('runtime_manifest.inventory',return_value={}).start()
        (self.guest/'.disposable-build').touch()
        for p,b in [(self.guest/'loader.img',b'fixture'),(self.guest/'desktop-apps.system',b'<system/>'),(self.guest/'runtime.json',b'{}'),(self.root/'source.c',b'source')]:p.write_bytes(b)
        self.manifest=dict(saved_data_used=False,development_storage_key=True,browser=True,virgl=True,desktop_resize=True,
            loader_sha256=desktop.digest(self.guest/'loader.img'),system_sha256=desktop.digest(self.guest/'desktop-apps.system'),runtime_sha256=desktop.digest(self.guest/'runtime.json'),sources={'source.c':desktop.digest(self.root/'source.c')})
        self.state=dict(saved_data_used=False,uuid='TEST',bundle=str(self.bundle),loader_sha256=self.manifest['loader_sha256'])
        self.config=dict(Information=dict(UUID='TEST'),Drive=[dict(ImageName='blank.img')],System=dict(CPUCount=4),QEMU=dict(Hypervisor=True))
        self.save()
        (self.data/'loader.img').write_bytes(b'fixture');(self.data/'blank.img').write_bytes(b'disposable')
        (self.data/'build-manifest.json').write_text(json.dumps(self.manifest))

    def save(self):
        (self.guest/'build.json').write_text(json.dumps(self.manifest))
        (self.guest/'session.json').write_text(json.dumps(self.state))
        (self.bundle/'config.plist').write_bytes(plistlib.dumps(self.config))

    def test_matching_disposable_session(self):
        self.assertEqual(desktop.session(self.guest)[3]['uuid'],'TEST')

    def test_wrapped_profile_requires_no_development_key(self):
        self.manifest['wrapped_session']=True;self.save()
        with self.assertRaisesRegex(ValueError,'profile'):desktop.built(self.guest)
        self.manifest['development_storage_key']=False;self.save()
        self.assertTrue(desktop.built(self.guest)[1]['wrapped_session'])

    def test_install_cannot_change_existing_disk_key_format(self):
        self.manifest.update(wrapped_session=True,development_storage_key=False);self.save()
        with self.assertRaisesRegex(ValueError,'different vault/key format'):
            desktop.session(self.guest,installed=False)

    def test_saved_data_session_is_rejected(self):
        self.state['saved_data_used']=True;self.save()
        with self.assertRaisesRegex(ValueError,'disposable VM'):desktop.session(self.guest)

    def test_existing_disk_name_is_rejected(self):
        self.config['Drive'][0]['ImageName']='wallet.img';self.save()
        with self.assertRaisesRegex(ValueError,'blank.img'):desktop.session(self.guest)

    def test_foreign_vm_identity_is_rejected(self):
        self.config['Information']['UUID']='REGULAR';self.save()
        with self.assertRaisesRegex(ValueError,'identity'):desktop.session(self.guest)

    def test_stale_running_loader_is_rejected(self):
        (self.data/'loader.img').write_bytes(b'older build')
        with self.assertRaisesRegex(ValueError,'installed first'):desktop.session(self.guest)

    def test_changed_source_and_system_are_rejected(self):
        (self.root/'source.c').write_bytes(b'new source')
        with self.assertRaisesRegex(ValueError,'source changed'):desktop.built(self.guest)
        (self.root/'source.c').write_bytes(b'source')
        (self.guest/'desktop-apps.system').write_bytes(b'<changed/>')
        with self.assertRaisesRegex(ValueError,'system or runtime'):desktop.built(self.guest)

    def test_linked_disk_is_rejected(self):
        (self.data/'blank.img').unlink();(self.data/'blank.img').symlink_to(self.root/'saved.img')
        with self.assertRaisesRegex(ValueError,'symbolic links'):desktop.session(self.guest)

    def test_run_cannot_replace_tracking_for_an_existing_vm(self):
        with patch.object(desktop,'run') as run:
            with self.assertRaisesRegex(ValueError,'already has a VM session'):
                desktop.launch(self.guest,self.root/'template.plist',True)
            run.assert_not_called()

    def test_fresh_run_forwards_the_actual_runner_option(self):
        (self.guest/'session.json').unlink()
        with patch.object(desktop,'run') as run:
            desktop.launch(self.guest,self.root/'template.plist',True)
            self.assertIn('--leave-running',run.call_args.args)
            self.assertNotIn('--keep-running',run.call_args.args)


if __name__=='__main__':unittest.main()
