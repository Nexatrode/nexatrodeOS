"""Admit exactly the five-page pre-AUTH experiment and reject broader authority."""
import copy
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET
from tests.sunfish_modem_data_probe_test import PORT,load
class GlinkPrepareProbeTests(unittest.TestCase):
    def test_exact_five_page_mode_has_no_aliases_irq_or_extra_writable_smem(self):
        audit=load('audit-capdl-memory');system=PORT/'modem-glink-prepare-probe.system'
        self.assertEqual(len(audit.fixed_modem_image_pages(system,False,True,True)),76)
        for observer,writer,prepare in [(False,False,True),(True,True,True),(False,True,False),(False,False,False)]:
            with self.assertRaises(ValueError):audit.fixed_modem_image_pages(system,observer,writer,prepare)
        with self.assertRaises(ValueError):audit.fixed_modem_image_pages(PORT/'modem-smp2p-probe.system',False,True,True)
        root=ET.parse(system).getroot()
        changes=[("memory_region[@name='glink_prepare_tail']",'phys_addr','0x8611f000'),
                 ("memory_region[@name='glink_prepare_tail']",'size','0x5000'),
                 (".//map[@mr='smem_snapshot_high']",'perms','rw'),
                 (".//map[@mr='glink_prepare_tail']",'cached','true')]
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'bad.system'
            for selector,key,value in changes:
                bad=copy.deepcopy(root);bad.find(selector).set(key,value);ET.ElementTree(bad).write(path)
                with self.assertRaises(ValueError):audit.fixed_modem_image_pages(path,False,True,True)
            for extra in [ET.Element('irq',irq='481',id='0'),ET.Element('map',mr='glink_prepare_tail',vaddr='0x45000000',perms='rw')]:
                bad=copy.deepcopy(root);bad.find('protection_domain').append(extra);ET.ElementTree(bad).write(path)
                with self.assertRaises(ValueError):audit.fixed_modem_image_pages(path,False,True,True)
    def test_prepare_mode_cannot_mix_receive_or_omit_the_fixed_status_profile(self):
        valid=dict(SUNFISH_GLINK_PREPARE_PROBE='1',SUNFISH_GLINK_RECEIVE_PROBE='0',SUNFISH_GLINK_BOOT_PROBE='0',SUNFISH_GLINK_VERSION_PROBE='0',SUNFISH_GLINK_PENDING_PROBE='0',SUNFISH_GLINK_OPEN_PROBE='0',SUNFISH_GLINK_CHANNEL_PROBE='0',SUNFISH_GLINK_INTENTS_PROBE='0',SUNFISH_GLINK_RX_PACKET_PROBE='0',SUNFISH_GLINK_DONE_PROBE='0',SUNFISH_QRTR_HELLO_PROBE='0',SUNFISH_QRTR_NEXT_PROBE='0',SUNFISH_QRTR_DISCOVERY_PROBE='0',SUNFISH_QRTR_CREDIT_PROBE='0',SUNFISH_QRTR_WAIT_PROBE='0',SUNFISH_MODEM_CRASH_PROBE='0',SUNFISH_MODEM_IMAGE_BIN='rust_sunfish_modem_smp2p_probe',
                   SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS='1',SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY='0',SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM='0',
                   SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE='0',SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC='0',SUNFISH_IMAGE_LOADER_CHECKPOINTS='1',
                   SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION='1',SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION='0',SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY='0',
                   SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS='1',SUNFISH_IMAGE_LOADER_TASK_IMAGES='1')
        for key,value in [('SUNFISH_GLINK_RECEIVE_PROBE','1'),('SUNFISH_GLINK_BOOT_PROBE','1'),('SUNFISH_GLINK_VERSION_PROBE','1'),('SUNFISH_GLINK_PENDING_PROBE','1'),('SUNFISH_GLINK_PENDING_PROBE','2'),('SUNFISH_GLINK_OPEN_PROBE','1'),('SUNFISH_GLINK_OPEN_PROBE','2'),('SUNFISH_GLINK_CHANNEL_PROBE','1'),('SUNFISH_GLINK_CHANNEL_PROBE','2'),('SUNFISH_GLINK_INTENTS_PROBE','1'),('SUNFISH_GLINK_INTENTS_PROBE','2'),('SUNFISH_GLINK_RX_PACKET_PROBE','1'),('SUNFISH_GLINK_RX_PACKET_PROBE','2'),('SUNFISH_GLINK_DONE_PROBE','1'),('SUNFISH_GLINK_DONE_PROBE','2'),('SUNFISH_QRTR_HELLO_PROBE','1'),('SUNFISH_QRTR_HELLO_PROBE','2'),('SUNFISH_QRTR_NEXT_PROBE','1'),('SUNFISH_QRTR_NEXT_PROBE','2'),('SUNFISH_QRTR_DISCOVERY_PROBE','1'),('SUNFISH_QRTR_DISCOVERY_PROBE','2'),('SUNFISH_QRTR_CREDIT_PROBE','1'),('SUNFISH_QRTR_CREDIT_PROBE','2'),('SUNFISH_QRTR_WAIT_PROBE','1'),('SUNFISH_QRTR_WAIT_PROBE','2'),('SUNFISH_MODEM_CRASH_PROBE','1'),('SUNFISH_MODEM_CRASH_PROBE','2'),('SUNFISH_GLINK_PREPARE_PROBE','2'),('SUNFISH_MODEM_IMAGE_BIN','rust_sunfish_modem_ready_probe'),('SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS','0')]:
            env=dict(os.environ,**valid);env[key]=value
            r=subprocess.run(['sh',str(PORT/'build-modem-image-probe.sh')],env=env,capture_output=True)
            self.assertNotEqual(r.returncode,0,key);self.assertEqual(r.stdout,b'',key)
if __name__=='__main__':unittest.main()
