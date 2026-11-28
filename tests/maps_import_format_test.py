#!/usr/bin/env python3
"""Host checks for local-pack identity, layout and real guest crash recovery."""
import hashlib,importlib.util,json,struct,subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('local_map_pack',ROOT/'experiments/comaps-native/local_pack.py');pack=importlib.util.module_from_spec(spec);spec.loader.exec_module(pack)
class LocalPacks(unittest.TestCase):
    def test_header_and_identity(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'source.mwm';p.write_bytes(b'boundedtestsource'*12)
            b=pack.pack(p,'Local Gibraltar',3,1)
            self.assertEqual(b[:8],b'NXMRPK01');self.assertEqual(b[256:288],hashlib.sha256(b[:256]).digest());self.assertEqual(b[112:144],hashlib.sha256(p.read_bytes()).digest())
            self.assertEqual(b[32:48],pack.identity('Local Gibraltar'));self.assertNotEqual(b[32:48],pack.identity('Gibraltar'))
            for region,version in [(2,1),(4,1),(3,0),(3,2**64-1)]:
                with self.assertRaises(AssertionError):pack.pack(p,'Local Gibraltar',region,version)
    def test_data_layout_excludes_private(self):
        ranges=[(64*1024*1024+(r*2+b)*pack.LIMIT,pack.LIMIT) for r in range(4) for b in range(2)]
        ranges += [(pack.INBOX,4096+pack.LIMIT),(pack.CATALOG,8192)]
        for i,(a,n) in enumerate(ranges):
            self.assertFalse(a<1024*1024+8192 and 1024*1024<a+n)
            for b,m in ranges[:i]:self.assertFalse(a<b+m and b<a+n)
    def test_guest_state_machine_under_sanitizers(self):
        out=ROOT/'build/maps-import-test'
        subprocess.run(['clang','-O1','-g','-fsanitize=address,undefined',str(ROOT/'tests/maps_import_test.c'),str(ROOT/'experiments/comaps-native/hash.c'),'-o',str(out)],check=True)
        subprocess.run([str(out)],check=True)
if __name__=='__main__':unittest.main()
