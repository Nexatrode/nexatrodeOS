import os,tempfile,unittest
from pathlib import Path
from desktop_boot_ready import APP_MAGIC,ready
from utm_vault_key_storage import QmpPipe

class BootReadyTest(unittest.TestCase):
    def state(self):
        gpu=[0]*32;gpu[0]=4;gpu[2]=2;gpu[9:11]=[2624,1644];gpu[13]=0x4d4f5431
        return gpu
    def test_rendered_apps_do_not_require_serial_markers(self):
        self.assertTrue(ready(self.state(),4,APP_MAGIC,APP_MAGIC))
    def test_zero_partial_or_torn_publication_is_not_ready(self):
        for field,value in ((0,0),(0,3),(2,1),(9,0),(10,0),(13,0)):
            gpu=self.state();gpu[field]=value
            self.assertFalse(ready(gpu,gpu[0],APP_MAGIC,APP_MAGIC))
        self.assertFalse(ready(self.state(),6,APP_MAGIC,APP_MAGIC))
        self.assertFalse(ready(self.state(),4,0,APP_MAGIC))
        self.assertFalse(ready(self.state(),4,APP_MAGIC,0))
    def test_one_reply_reader_and_lock_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'monitor';os.mkfifo(str(path)+'.out')
            first=QmpPipe(path)
            try:
                with self.assertRaisesRegex(RuntimeError,'another client'):QmpPipe(path)
            finally:first.close()
            first.close() # cleanup paths may close an already released client
            second=QmpPipe(path);second.close()

if __name__=='__main__':unittest.main()
