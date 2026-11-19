"""Optional native ramdisk preserves the Android-v2 DTB and default behavior."""
import hashlib
import importlib.util
import struct
import unittest
from pathlib import Path

PORT = Path(__file__).resolve().parents[1] / 'platforms/sunfish'
spec = importlib.util.spec_from_file_location('boot_v2', PORT / 'repack-boot-v2.py')
pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pack)

class BootPackerTests(unittest.TestCase):
    def reference(self):
        header = bytearray(4096)
        header[:8] = b'ANDROID!'
        for offset, value in [(8,3),(16,5),(36,4096),(40,2),(1644,1660),(1648,4)]:
            struct.pack_into('<I',header,offset,value)
        return bytes(header) + b'old'.ljust(4096,b'\0') + b'prior'.ljust(4096,b'\0') + b'\xd0\x0d\xfe\xed'.ljust(4096,b'\0')

    def test_default_retains_original_ramdisk_and_dtb(self):
        header, parts, _ = pack.components(pack.pack(self.reference(),b'newkernel'))
        self.assertEqual(parts['kernel'],b'newkernel')
        self.assertEqual(parts['ramdisk'],b'prior')
        self.assertEqual(parts['dtb'],b'\xd0\x0d\xfe\xed')
        self.assertEqual(pack.u32(header,16),5)

    def test_override_updates_sizes_and_boot_id_without_changing_dtb(self):
        header, parts, _ = pack.components(pack.pack(self.reference(),b'newkernel',b'minimal'))
        self.assertEqual(parts['ramdisk'],b'minimal')
        self.assertEqual(pack.u32(header,16),7)
        self.assertEqual(parts['dtb'],b'\xd0\x0d\xfe\xed')
        digest=hashlib.sha1()
        for name in ('kernel','ramdisk','second','recovery_dtbo','dtb'):
            digest.update(parts[name]);digest.update(struct.pack('<I',len(parts[name])))
        self.assertEqual(header[576:608],digest.digest().ljust(32,b'\0'))
        with self.assertRaises(ValueError):pack.pack(self.reference(),b'newkernel',b'')

    def test_appended_cmdline_is_space_delimited_across_v2_fields(self):
        reference = bytearray(self.reference())
        base_cmdline = b'console=ttyMSM0 cgroup_disable=pressure'
        reference[64:576] = base_cmdline + b'\0' * (512 - len(base_cmdline))
        argument = b'dyndbg="func update_request_adhoc +p"'
        packed = pack.pack(bytes(reference), b'old', append_cmdline=argument)
        header, parts, _ = pack.components(packed)
        actual_cmdline = (
            bytes(header[64:576]).split(b'\0', 1)[0]
            + bytes(header[608:1632]).split(b'\0', 1)[0]
        )
        self.assertEqual(actual_cmdline, base_cmdline + b' ' + argument)
        self.assertEqual(parts['kernel'], b'old')
        self.assertEqual(parts['dtb'], b'\xd0\x0d\xfe\xed')

    def test_appended_cmdline_preserves_existing_extra_text_and_rejects_nul(self):
        reference = bytearray(self.reference())
        reference[64:576] = b'base=1\0' + bytes(505)
        reference[608:1632] = b' extra=2\0' + bytes(1015)
        packed = pack.pack(bytes(reference), b'kernel', append_cmdline='next=3')
        header, _, _ = pack.components(packed)
        self.assertEqual(
            bytes(header[64:576]).split(b'\0', 1)[0]
            + bytes(header[608:1632]).split(b'\0', 1)[0],
            b'base=1 extra=2 next=3',
        )
        with self.assertRaises(ValueError):
            pack.pack(bytes(reference), b'kernel', append_cmdline=b'bad\0arg')

    def test_wrapper_accepts_adrp_x1_stack_displacements_and_rejects_other_entries(self):
        spec=importlib.util.spec_from_file_location('wrapper',PORT/'wrap-microkit-image.py')
        wrapper=importlib.util.module_from_spec(spec);spec.loader.exec_module(wrapper)
        for first in [0xb0000021,0x90000001,0xf000ffe1]:
            loader=struct.pack('<I',first)+bytes(60)
            self.assertEqual(wrapper.wrap(loader)[4096:],loader)
        for first in [0,0x14000000,0x10000001,0x90000000]:
            with self.assertRaises(ValueError):wrapper.wrap(struct.pack('<I',first)+bytes(60))

if __name__ == '__main__':unittest.main()
