import copy
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET
from tests.sunfish_modem_data_probe_test import PORT,load

class FatalTraceProbeTests(unittest.TestCase):
    def test_fatal_trace_preserves_existing_transport_manifest_without_gicd(self):
        audit=load('audit-capdl-memory')
        system=PORT/'modem-fatal-trace-probe.system'
        self.assertEqual(len(audit.fixed_modem_image_pages(system,False,True,True,True)),76)
        allowed=audit.fixed_modem_fatal_trace_pages(system,True)
        self.assertEqual(allowed,set())
        self.assertFalse(ET.parse(system).getroot().find("memory_region[@name='gicd_watchdog_status']"))
        self.assertFalse(ET.parse(system).getroot().find(".//map[@mr='gicd_watchdog_status']"))
        self.assertEqual(audit.validate_modem_fatal_trace_frames([],allowed),[])
        with self.assertRaises(ValueError):audit.fixed_modem_image_pages(system,False,False,True,True)
        with self.assertRaises(ValueError):audit.validate_modem_fatal_trace_frames([(0x17a00000,0x1000,'frame_mr_gicd_watchdog_status_0')],allowed)
        original=ET.parse(system).getroot()
        changes=[(".//map[@mr='glink_prepare_tail']",'perms','r'),
                 (".//map[@mr='glink_prepare_tail']",'cached','true'),
                 (".//map[@mr='glink_prepare_tail']",'vaddr','0x44123000'),
                 ("memory_region[@name='glink_prepare_tail']",'phys_addr','0x86123000'),
                 ("memory_region[@name='glink_prepare_tail']",'size','0x5000')]
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'bad.system'
            for selector,key,value in changes:
                changed=copy.deepcopy(original);changed.find(selector).set(key,value);ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError):audit.fixed_modem_image_pages(path,False,True,True,True)

    def test_probe_rejects_trace_without_existing_bounded_crash_run(self):
        env=dict(os.environ,SUNFISH_MODEM_FATAL_TRACE_PROBE='1',SUNFISH_MODEM_CRASH_PROBE='0')
        result=subprocess.run(['sh',str(PORT/'build-modem-image-probe.sh')],env=env,capture_output=True)
        self.assertNotEqual(result.returncode,0)
        self.assertEqual(result.stdout,b'')

    def test_fatal_trace_gets_its_own_exact_loader_task_manifest(self):
        prepare=load('prepare-image-loader-sdk')
        self.assertEqual(prepare.task_manifest_path(False,False,False,True,True).name,
                         'loader-task-regions-fatal-trace.json')
        self.assertEqual(prepare.task_manifest_path(False,False,False,True).name,
                         'loader-task-regions-smp2p.json')
        with self.assertRaises(AssertionError):prepare.task_manifest_path(False,False,False,False,True)

    def test_static_source_and_native_probe_proof(self):
        result=subprocess.run(['python3',str(PORT/'validate-modem-fatal-trace-inputs.py')],
                              cwd=PORT.parents[1],capture_output=True,text=True,check=True)
        import json
        proof=json.loads(result.stdout)
        self.assertEqual(proof['modem_wdog_gic_spi'],266)
        self.assertIn('not mapped',proof['gicd_mapping'])
        self.assertEqual(proof['new_gicd_writes'],0)
        self.assertEqual(proof['added_protocol_or_smem_writes'],0)

if __name__=='__main__':unittest.main()
