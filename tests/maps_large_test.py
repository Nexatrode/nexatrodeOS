#!/usr/bin/env python3
"""Real public paged region, bounded guest installer and malformed directories."""
import importlib.util,json,subprocess,unittest,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('maps_paged_pack',ROOT/'experiments/comaps-native/paged_pack.py');pack=importlib.util.module_from_spec(spec);spec.loader.exec_module(pack)
class LargeRegions(unittest.TestCase):
 def test_real_region_stream_and_recovery(self):
  out=ROOT/'build/maps-large-host';out.mkdir(parents=True,exist_ok=True)
  sys.path.insert(0,str(ROOT/'experiments/comaps-native'));from prepare import prepare,prepare_storage
  (out/'.disposable-build').touch();prepare(ROOT/'build/comaps');prepare_storage(ROOT/'build/comaps',out)
  src=out/'spatial';profile=json.loads((src/'Liechtenstein-profile.json').read_text())
  info=pack.package(src/'Liechtenstein.nxmap',src/'Liechtenstein.nxsearch',profile,'Local Liechtenstein',3,1,out/'large.nxregion');self.assertGreater(info['bytes'],8*1024*1024);self.assertLessEqual(info['largest_page'],8*1024*1024)
  subprocess.run(['clang','-O1','-g','-fsanitize=address,undefined',str(ROOT/'tests/maps_large_store_test.c'),str(ROOT/'experiments/comaps-native/hash.c'),'-o',str(out/'test')],check=True)
  subprocess.run([str(out/'test'),str(out/'large.nxregion')],check=True)
if __name__=='__main__':unittest.main()
