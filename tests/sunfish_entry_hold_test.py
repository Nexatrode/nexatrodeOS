"""Entry diagnostic must stop before it can fall into the appended payload."""
import struct
import unittest
from tests.sunfish_modem_data_probe_test import load


class EntryHoldTests(unittest.TestCase):
    def setUp(self):
        self.check = load('build-entry-hold-check').validate_entry
        self.image = bytearray(struct.pack('<7I', 0x90000001, 0x91000021,
                                          0x9100003f, 0xd5034fdf, 0x10000000,
                                          0x94000003, 0x14000000) + bytes(36))

    def test_no_fallthrough_or_outside_drawing_target(self):
        self.check(self.image, 0, 24)
        for offset, word in [(24, 0x14000001), (24, 0xd503201f),
                             (20, 0x940000ff), (20, 0x97fffffb),
                             (16, 0x10000001), (12, 0xd503201f)]:
            image = self.image.copy()
            struct.pack_into('<I', image, offset, word)
            with self.assertRaises(ValueError):
                self.check(image, 0, 24)

    def test_entry_and_hold_must_be_at_pinned_offsets(self):
        for entry, hold in [(4, 24), (0, 28), (0, 0)]:
            with self.assertRaises(ValueError):
                self.check(self.image, entry, hold)


if __name__ == '__main__':
    unittest.main()
