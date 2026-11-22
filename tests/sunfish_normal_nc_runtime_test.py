#!/usr/bin/env python3
"""Exercise opt-in gate before the unchanged authority audit."""
import copy,importlib.util,runpy,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('nc_runtime_audit',ROOT/'platforms/sunfish/validate-native-mpss-runtime.py');audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
fixture=runpy.run_path(str(ROOT/'tests/sunfish_table_memory_profile_test.py'))['fixture']
class RuntimeMemoryGate(unittest.TestCase):
    def run_profile(self,enabled,explicit):
        root,model=fixture(enabled);before=copy.deepcopy(model)
        if not enabled:
            for e in model['objects'][11]['object']['PageTable']['slots']:e['cap']['Frame']['normal_nc']=False
            before=copy.deepcopy(model)
        seen=[]
        def authority(system,projected,report,dtbs):
            self.assertFalse(any('normal_nc' in m.attrib for m in ET.parse(system).getroot().iter('map')))
            for e in projected['objects'][11]['object']['PageTable']['slots']:
                self.assertNotIn('normal_nc',e['cap']['Frame'])
                self.assertFalse(e['cap']['Frame']['cached'])
            seen.append(True);return {'authority_passed':True}
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'profile.system';ET.ElementTree(root).write(p)
            with patch.object(audit,'_audit_authority',authority,create=True):
                if enabled and not explicit:
                    with self.assertRaises(ValueError):audit.audit(p,model,'unused')
                    self.assertEqual(seen,[])
                else:
                    result=audit.audit(p,model,'unused',table_normal_nc=explicit)
                    self.assertTrue(result['authority_passed']);self.assertEqual(seen,[True])
                    self.assertEqual(result['table_memory_profile']['normal_nc_frames'],8 if enabled else 0)
            self.assertEqual(model,before);self.assertEqual(ET.tostring(root),ET.tostring(ET.parse(p).getroot()))
    def test_experiment_requires_explicit_mode(self):self.run_profile(True,False)
    def test_explicit_mode_gates_then_projects(self):self.run_profile(True,True)
    def test_default_accepts_false_fields_from_extended_tool(self):self.run_profile(False,False)
if __name__=='__main__':unittest.main()
