"""A separate mode confines the AP status writer to one fixed SMEM page."""
import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET
from tests.sunfish_modem_data_probe_test import PORT,load

class Smp2pProbeTests(unittest.TestCase):
    def test_servreg_probe_uses_its_own_pinned_task_manifest(self):
        prepare=load('prepare-image-loader-sdk')
        normal=prepare.task_manifest_path(False,False,False,True)
        servreg=prepare.task_manifest_path(False,False,False,True,False,True)
        pdr_reply=prepare.task_manifest_path(False,False,False,True,False,True,qrtr_servreg_pdr_reply=True)
        self.assertEqual(normal.name,'loader-task-regions-smp2p.json')
        self.assertEqual(servreg.name,'loader-task-regions-qrtr-servreg.json')
        self.assertEqual(pdr_reply.name,'loader-task-regions-qrtr-pdr-reply.json')
        self.assertNotEqual(normal,servreg)
        self.assertNotEqual(pdr_reply,servreg)
        manifest=json.loads(servreg.read_text())
        self.assertEqual((len(manifest['header']),len(manifest['regions']),len(manifest['native_pd_elf_sha256'])),(8,11,64))
        with self.assertRaises(AssertionError):
            prepare.task_manifest_path(False,False,False,False,False,True)

    def test_exact_split_mapping_and_explicit_writer_mode(self):
        audit=load('audit-capdl-memory');system=PORT/'modem-smp2p-probe.system'
        self.assertEqual(len(audit.fixed_modem_image_pages(system,False,True)),76)
        for observer,writer in [(False,False),(True,False),(True,True)]:
            with self.assertRaises(ValueError):audit.fixed_modem_image_pages(system,observer,writer)
        original=ET.parse(system).getroot()
        changes=[(".//map[@mr='smem_snapshot_low']",'perms','rw'),
                 (".//map[@mr='smem_snapshot_high']",'perms','rw'),
                 ("memory_region[@name='smem_ap_page']",'size','0x2000'),
                 ("memory_region[@name='smem_ap_page']",'phys_addr','0x8611e000'),
                 (".//map[@mr='smem_ap_page']",'cached','true')]
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'bad.system'
            for selector,key,value in changes:
                changed=copy.deepcopy(original);changed.find(selector).set(key,value);ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError):audit.fixed_modem_image_pages(path,False,True)
            for extra in [ET.Element('irq',irq='483',id='0'),ET.Element('map',mr='smem_ap_page',vaddr='0x45000000',perms='rw')]:
                changed=copy.deepcopy(original);changed.find('protection_domain').append(extra);ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError):audit.fixed_modem_image_pages(path,False,True)

    def test_separate_plan_and_builder_rejection_of_mixed_or_incomplete_modes(self):
        prepare=load('prepare-image-loader-sdk')
        self.assertEqual(prepare.task_manifest_path(False,False,False,True).name,'loader-task-regions-smp2p.json')
        for flags in [(True,False,False),(False,True,False),(False,False,True)]:
            with self.assertRaises(AssertionError):prepare.task_manifest_path(*flags,True)
        valid=dict(SUNFISH_MODEM_IMAGE_BIN='rust_sunfish_modem_smp2p_probe',
                   SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS='1',SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY='0',
                   SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM='0',SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE='0',SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC='0',
                   SUNFISH_IMAGE_LOADER_CHECKPOINTS='1',SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION='0',SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY='0',
                   SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION='1',SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS='1',SUNFISH_IMAGE_LOADER_TASK_IMAGES='1')
        for key,value in [('SUNFISH_MODEM_IMAGE_BIN','rust_sunfish_modem_ready_probe'),('SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS','0'),
                          ('SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY','1'),('SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM','1'),
                          ('SUNFISH_IMAGE_LOADER_TASK_IMAGES','0'),('SUNFISH_IMAGE_LOADER_CHECKPOINTS','0')]:
            env=dict(os.environ,**valid);env[key]=value
            r=subprocess.run(['sh',str(PORT/'build-modem-image-probe.sh')],env=env,capture_output=True)
            self.assertNotEqual(r.returncode,0,key);self.assertEqual(r.stdout,b'',key)
if __name__=='__main__':unittest.main()
