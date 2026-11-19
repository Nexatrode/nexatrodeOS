"""The passive status mode admits only its exact two added mappings."""
import copy
import os
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET
from tests.sunfish_modem_data_probe_test import PORT, load

class ReadyProbeTests(unittest.TestCase):
    def test_status_mapping_requires_explicit_mode_and_stays_read_only(self):
        audit=load('audit-capdl-memory')
        system=PORT/'modem-ready-probe.system'
        self.assertEqual(len(audit.fixed_modem_image_pages(system,True)),76)
        with self.assertRaises(ValueError):audit.fixed_modem_image_pages(system)
        with self.assertRaises(ValueError):audit.fixed_modem_image_pages(PORT/'modem-image-probe.system',True)
        original=ET.parse(system).getroot()
        changes=[(".//map[@mr='smem_snapshot']",'perms','rw'),
                 (".//map[@mr='smem_snapshot']",'cached','true'),
                 ("memory_region[@name='smem_snapshot']",'phys_addr','0x86200000'),
                 ("memory_region[@name='tcsr_smem_lock']",'phys_addr','0x01f42000'),
                 ("memory_region[@name='tcsr_smem_lock']",'size','0x2000')]
        with tempfile.TemporaryDirectory() as d:
            path=__import__('pathlib').Path(d)/'changed.system'
            for selector,key,value in changes:
                changed=copy.deepcopy(original);changed.find(selector).set(key,value)
                ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError):audit.fixed_modem_image_pages(path,True)
            for extra in [ET.Element('irq',irq='483',id='0'),ET.Element('map',mr='smem_snapshot',vaddr='0x45000000',perms='rw')]:
                changed=copy.deepcopy(original);changed.find('protection_domain').append(extra)
                ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError):audit.fixed_modem_image_pages(path,True)

    def test_ready_mode_has_separate_manifest_and_cannot_combine_start_modes(self):
        prepare=load('prepare-image-loader-sdk')
        self.assertEqual(prepare.task_manifest_path(False,False,True).name,'loader-task-regions-ready.json')
        for stage,auth in [(True,False),(False,True),(True,True)]:
            with self.assertRaises(AssertionError):prepare.task_manifest_path(stage,auth,True)

    def test_builder_rejects_wrong_program_mixed_modes_and_incomplete_loader_path(self):
        valid=dict(SUNFISH_MODEM_IMAGE_BIN='rust_sunfish_modem_ready_probe',
                   SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS='0',
                   SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY='1',
                   SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM='0',
                   SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE='0',
                   SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC='0',
                   SUNFISH_IMAGE_LOADER_CHECKPOINTS='1',
                   SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION='0',
                   SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY='0',
                   SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION='1',
                   SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS='1',
                   SUNFISH_IMAGE_LOADER_TASK_IMAGES='1')
        for key,value in [('SUNFISH_MODEM_IMAGE_BIN','rust_sunfish_modem_auth_probe'),
                          ('SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM','1'),
                          ('SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY','0'),
                          ('SUNFISH_IMAGE_LOADER_TASK_IMAGES','0'),
                          ('SUNFISH_IMAGE_LOADER_CHECKPOINTS','0'),
                          ('SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION','0')]:
            env=dict(os.environ,**valid);env[key]=value
            result=subprocess.run(['sh',str(PORT/'build-modem-image-probe.sh')],env=env,capture_output=True)
            self.assertNotEqual(result.returncode,0,key)
            self.assertEqual(result.stdout,b'',key)

if __name__=='__main__':unittest.main()
