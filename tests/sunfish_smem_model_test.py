import struct
import unittest

from platforms.sunfish.smem_model import (
    SMEM_PRIVATE_CANARY,
    SmemFormatError,
    resolve_private_uncached_item,
)


REGION_SIZE = 0x200000
PART_OFFSET = 0x10000
PART_SIZE = 0x1000
PTABLE_OFFSET = REGION_SIZE - 0x1000


def fixture():
    data = bytearray(REGION_SIZE)
    struct.pack_into("<II", data, 192, 1, 0)  # initialized, free_offset (overwritten below)
    struct.pack_into("<I", data, 204, 0)  # reserved
    struct.pack_into("<I", data, 64 + 4 * 7, 12 << 16)  # SBL reports SMEM v12
    data[PTABLE_OFFSET:PTABLE_OFFSET + 4] = b"$TOC"
    struct.pack_into("<II", data, PTABLE_OFFSET + 4, 1, 1)
    entry = PTABLE_OFFSET + 32
    struct.pack_into("<IIIHHI", data, entry, PART_OFFSET, PART_SIZE, 0, 0, 1, 64)
    # Seven reserved words complete the 48-byte table entry.
    part = PART_OFFSET
    data[part:part + 4] = b"$PRT"
    struct.pack_into("<HHIIII", data, part + 4, 0, 1, PART_SIZE, 104, 104, 0)
    for index, item_id in enumerate((478, 479, 480)):
        header = part + 32 + index * 24
        struct.pack_into("<HHIHHI", data, header, SMEM_PRIVATE_CANARY,
                         item_id, 8, 0, 0, 0)
        payload = header + 16
        data[payload:payload + 8] = bytes([item_id & 0xff]) * 8
    return data


class SmemPrivateItemTests(unittest.TestCase):
    def test_resolves_expected_glink_items_in_modem_partition(self):
        data = fixture()
        for item_id in (478, 479, 480):
            with self.subTest(item=item_id):
                item = resolve_private_uncached_item(data, item_id)
                self.assertEqual(item.item_id, item_id)
                self.assertEqual(item.size, 8)
                self.assertEqual((item.host0, item.host1), (0, 1))
                self.assertEqual(data[item.offset], item_id & 0xff)

    def test_rejects_bad_table_count_and_partition_bounds(self):
        data = fixture()
        struct.pack_into("<I", data, 64 + 4 * 7, 11 << 16)
        with self.assertRaisesRegex(SmemFormatError, "version"):
            resolve_private_uncached_item(data, 478)

        data = fixture()
        struct.pack_into("<I", data, PTABLE_OFFSET + 8, 0xffffffff)
        with self.assertRaisesRegex(SmemFormatError, "exceed"):
            resolve_private_uncached_item(data, 478)

        data = fixture()
        struct.pack_into("<I", data, PTABLE_OFFSET + 32, REGION_SIZE)
        with self.assertRaisesRegex(SmemFormatError, "outside"):
            resolve_private_uncached_item(data, 478)

    def test_rejects_corrupt_allocation_and_does_not_guess_cached_items(self):
        data = fixture()
        struct.pack_into("<H", data, PART_OFFSET + 32, 0)
        with self.assertRaisesRegex(SmemFormatError, "canary"):
            resolve_private_uncached_item(data, 478)

        data = fixture()
        with self.assertRaisesRegex(SmemFormatError, "not in the uncached"):
            resolve_private_uncached_item(data, 481)

    def test_resolves_cached_item_using_reverse_cacheline_walk(self):
        data = fixture()
        cached_header_offset = PART_SIZE - 64
        cached_header = PART_OFFSET + cached_header_offset
        cached_payload = cached_header - 8
        cached_free = cached_header_offset - 8 - 64
        struct.pack_into("<I", data, PART_OFFSET + 16, cached_free)
        struct.pack_into("<HHIHHI", data, cached_header,
                         SMEM_PRIVATE_CANARY, 481, 8, 0, 0, 0)
        data[cached_payload:cached_header] = b"GLINK123"

        item = resolve_private_uncached_item(data, 481, include_cached=True)
        self.assertEqual(item.offset, cached_payload)
        self.assertEqual(item.size, 8)
        self.assertEqual(data[item.offset:item.offset + item.size], b"GLINK123")

    def test_rejects_bad_cached_canary(self):
        data = fixture()
        cached_header_offset = PART_SIZE - 64
        cached_header = PART_OFFSET + cached_header_offset
        cached_free = cached_header_offset - 8 - 64
        struct.pack_into("<I", data, PART_OFFSET + 16, cached_free)
        struct.pack_into("<HHIHHI", data, cached_header, 0, 481, 8, 0, 0, 0)
        with self.assertRaisesRegex(SmemFormatError, "cached allocation canary"):
            resolve_private_uncached_item(data, 481, include_cached=True)


if __name__ == "__main__":
    unittest.main()
