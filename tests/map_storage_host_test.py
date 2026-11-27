"""Checksums against hashlib, two real regions and torn selection records."""
import ctypes,hashlib,importlib.util,json,subprocess,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];HERE=ROOT/'experiments/comaps-native'
class Storage(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.out=ROOT/'build/comaps/storage-host';cls.out.mkdir(parents=True,exist_ok=True)
        (cls.out/'.disposable-build').touch()
        spec=importlib.util.spec_from_file_location('maps_prepare_storage',HERE/'prepare.py');p=importlib.util.module_from_spec(spec);spec.loader.exec_module(p)
        cls.catalog=p.prepare_storage(ROOT/'build/comaps',cls.out)
        wrapper=cls.out/'wrapper.c';wrapper.write_text('#include "storage_format.h"\nint valid(const unsigned char*p){uint64_t g;unsigned r;return map_selection_read(p,&g,&r);}\nvoid record(unsigned char*p,unsigned long long g,unsigned r){map_selection_write(p,g,r);}\n')
        subprocess.run(['/opt/homebrew/opt/llvm/bin/clang','-dynamiclib','-I'+str(HERE),str(wrapper),str(HERE/'hash.c'),'-o',str(cls.out/'hash.dylib')],check=True)
        cls.lib=ctypes.CDLL(str(cls.out/'hash.dylib'))
        cls.lib.map_sha256.argtypes=[ctypes.c_void_p,ctypes.c_size_t,ctypes.c_void_p]
        cls.lib.valid.argtypes=[ctypes.c_void_p];cls.lib.record.argtypes=[ctypes.c_void_p,ctypes.c_ulonglong,ctypes.c_uint]
    def test_hash_vectors_and_boundaries(self):
        for n in [0,1,3,55,56,63,64,65,127,128,129,4096,65536,1000000]:
            raw=bytes((i*41+7)%256 for i in range(n));out=ctypes.create_string_buffer(32)
            self.lib.map_sha256(raw,n,out);self.assertEqual(out.raw,hashlib.sha256(raw).digest())
    def test_pinned_disk_files_and_guest_hash(self):
        pack=(self.out/'maps.img').read_bytes();self.assertEqual(len(pack),32*1024*1024)
        for i,r in enumerate(self.catalog['regions']):
            raw=pack[(i+1)*8*1024*1024:][:r['bytes']];out=ctypes.create_string_buffer(32)
            self.lib.map_sha256(raw,len(raw),out);self.assertEqual(out.raw.hex(),r['sha256'])
    def test_torn_and_corrupt_selection_records(self):
        record=ctypes.create_string_buffer(4096);self.lib.record(record,12,1);raw=record.raw
        self.assertTrue(self.lib.valid(raw))
        for cut in range(1,64):self.assertFalse(self.lib.valid(raw[:cut]+bytes(4096-cut)))
        for i in range(64):
            changed=bytearray(raw);changed[i]^=128;self.assertFalse(self.lib.valid(bytes(changed)))
        for generation,region in [(0,0),(2**64-1,0),(1,2)]:
            self.lib.record(record,generation,region);self.assertFalse(self.lib.valid(record))
    def test_real_regions_sanitized(self):
        exe=self.out/'reader';subprocess.run(['/opt/homebrew/opt/llvm/bin/clang','-O1','-g','-fsanitize=address,undefined','-fno-sanitize-recover=all','-I'+str(HERE),str(HERE/'mwm.c'),str(HERE/'reader_test.c'),'-o',str(exe)],check=True)
        for r in self.catalog['regions']:subprocess.run([str(exe),str(ROOT/'build/comaps'/(r['name']+'.mwm'))],check=True)
if __name__=='__main__':unittest.main()
