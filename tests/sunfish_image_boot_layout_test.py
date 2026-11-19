"""Range checks include the entire relocated payload, not just loader code."""
import unittest
from tests.sunfish_modem_data_probe_test import load

class BootLayoutTests(unittest.TestCase):
    def test_native_full_payload_overlap_is_rejected_even_with_disjoint_code(self):
        check = load('validate-image-boot-layout').validate_disjoint
        # The prefixes are separate, but the 70MiB copy would overwrite the
        # instructions still executing at b0081000 while the copy is in flight.
        check(0xb0080000,0xf010,0xb0000000,0xf010)
        with self.assertRaises(ValueError):
            check(0xb0080000,0x46533c4,0xb0000000,0x46523c4)
        check(0xb0080000,0x46533c4,0xc0000000,0x46523c4)
        check(0xb0080000,0x46533c4,0xb46d33c4,1)
        with self.assertRaises(ValueError):
            check(0xb0080000,0x46533c4,0xb46d33c3,1)

    def test_ram_bounds_and_partial_reserved_intersections(self):
        check = load('validate-image-boot-layout').validate_range
        ram = [(0x80000000,0x10000000)]
        reserved = [('fixed',0x85000000,0x100000,True)]
        check('ok',0x80000000,0x5000000,ram,reserved)
        check('ok',0x85100000,0x1000,ram,reserved)
        for start,size in [(0x7ffff000,0x2000),(0x8ffff000,0x2000),
                           (0x84fff000,0x2000),(0x85000000,1),(0x850ff000,0x2000),
                           (0x80000000,0x6000000),(0x80000000,0)]:
            with self.assertRaises(ValueError):check('bad',start,size,ram,reserved)

if __name__ == '__main__':unittest.main()
