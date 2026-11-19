import struct
import importlib.util
from pathlib import Path
import unittest

MODULE_PATH = (
    Path(__file__).resolve().parents[1]
    / "platforms/sunfish/parse-wmi-init-resource-dump.py"
)
SPEC = importlib.util.spec_from_file_location("sunfish_wmi_resource_dump", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
DumpError = MODULE.DumpError
parse_dump = MODULE.parse_dump

CAPTURED_SUNFISH_WMI_INIT = """\
[ 1187.191033] SUNFISH_WMI_INIT_RSRC 00000000: 10 01 4b 00 04 00 00 00 26 00 00 00 03 00 00 00
[ 1187.200607] SUNFISH_WMI_INIT_RSRC 00000010: 03 00 00 00 02 00 00 00 4c 00 00 00 10 00 00 00
[ 1187.210243] SUNFISH_WMI_INIT_RSRC 00000020: 03 00 00 00 03 00 00 00 64 00 00 00 64 00 00 00
[ 1187.219890] SUNFISH_WMI_INIT_RSRC 00000030: 64 00 00 00 28 00 00 00 02 00 00 00 04 00 00 00
[ 1187.229410] SUNFISH_WMI_INIT_RSRC 00000040: 03 00 00 00 03 00 00 00 08 00 00 00 00 00 00 00
[ 1187.238988] SUNFISH_WMI_INIT_RSRC 00000050: 00 00 00 00 00 00 00 00 00 04 00 00 00 00 00 00
[ 1187.248540] SUNFISH_WMI_INIT_RSRC 00000060: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
[ 1187.258112] SUNFISH_WMI_INIT_RSRC 00000070: 03 00 00 00 20 04 00 00 02 00 00 00 01 00 00 00
[ 1187.267664] SUNFISH_WMI_INIT_RSRC 00000080: 08 00 00 00 03 00 00 00 20 00 00 00 02 00 00 00
[ 1187.277239] SUNFISH_WMI_INIT_RSRC 00000090: 06 00 00 00 00 00 00 00 01 00 00 00 01 00 00 00
[ 1187.286805] SUNFISH_WMI_INIT_RSRC 000000a0: 00 00 00 00 01 00 00 00 02 00 00 00 02 00 00 00
[ 1187.296330] SUNFISH_WMI_INIT_RSRC 000000b0: 00 06 18 00 00 00 00 00 00 00 00 00 00 00 00 00
[ 1187.305840] SUNFISH_WMI_INIT_RSRC 000000c0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
[ 1187.315490] SUNFISH_WMI_INIT_RSRC 000000d0: 00 00 00 00 00 00 00 00 01 00 00 00 07 00 00 00
[ 1187.325015] SUNFISH_WMI_INIT_RSRC 000000e0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
[ 1187.334596] SUNFISH_WMI_INIT_RSRC 000000f0: 00 00 00 00 00 00 00 00 ff ff ff ff 00 00 00 00
[ 1187.344121] SUNFISH_WMI_INIT_RSRC 00000100: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
[ 1187.353672] SUNFISH_WMI_INIT_RSRC 00000110: 00 00 00 00
"""


def encode_dump(*, tag=75, value_length=272, total_length=276):
    raw = bytearray(total_length)
    if total_length >= 4:
        raw[:4] = struct.pack("<HH", value_length, tag)
    for index in range(4, total_length, 4):
        word = (index - 4) // 4 + 1
        raw[index : index + min(4, total_length - index)] = word.to_bytes(
            4, "little"
        )[: min(4, total_length - index)]
    lines = []
    for offset in range(0, len(raw), 16):
        chunk = raw[offset : offset + 16]
        encoded = " ".join(f"{byte:02x}" for byte in chunk)
        lines.append(f"[  2.500000] SUNFISH_WMI_INIT_RSRC {offset:08x}: {encoded}")
    return raw, "\n".join(lines)


class WmiInitResourceDumpTests(unittest.TestCase):
    def test_decodes_captured_lineage_sunfish_resource_config(self):
        result = parse_dump(CAPTURED_SUNFISH_WMI_INIT)

        self.assertEqual(result["tlv_tag"], 75)
        self.assertEqual(result["tlv_value_bytes"], 272)
        self.assertEqual(result["selected_fields"]["num_vdevs"], "0x00000004")
        self.assertEqual(result["selected_fields"]["num_peers"], "0x00000026")
        self.assertEqual(result["selected_fields"]["tx_chain_mask"], "0x00000003")
        self.assertEqual(result["selected_fields"]["rx_chain_mask"], "0x00000003")
        self.assertEqual(result["selected_fields"]["num_msdu_desc"], "0x00000420")
        self.assertEqual(result["selected_fields"]["max_frag_entries"], "0x00000002")
        self.assertEqual(result["resource_fields"]["num_wow_filters"], "0x00000002")
        self.assertEqual(result["resource_fields"]["flag1"], "0x00180600")
        self.assertEqual(result["resource_fields"]["use_pdev_id"], "0x00000001")
        self.assertEqual(
            result["resource_fields"]["num_packet_filters"], "0xffffffff"
        )

    def test_decodes_one_complete_resource_tlv_and_selected_fields(self):
        raw, text = encode_dump()
        result = parse_dump(text)

        self.assertEqual(raw[:4], bytes.fromhex("10 01 4b 00"))
        self.assertEqual(result["tlv_tag"], 75)
        self.assertEqual(result["tlv_value_bytes"], 272)
        self.assertEqual(result["tlv_bytes_hex"], raw.hex())
        self.assertEqual(len(result["words_le"]), 68)
        self.assertEqual(len(result["resource_fields"]), 68)
        self.assertEqual(result["selected_fields"]["num_vdevs"], "0x00000001")
        self.assertEqual(result["selected_fields"]["num_peers"], "0x00000002")
        self.assertEqual(result["resource_fields"]["rx_timeout_pri[3]"], "0x0000000d")
        self.assertEqual(result["resource_fields"]["flag1"], "0x0000002c")
        self.assertEqual(result["resource_fields"]["max_nlo_ssids"], "0x0000003d")
        self.assertEqual(result["resource_fields"]["num_packet_filters"], "0x0000003e")
        self.assertEqual(
            result["selected_fields"]["num_max_sta_vdevs"], "0x0000003f"
        )
        self.assertEqual(result["resource_fields"]["flags2"], "0x00000044")

    def test_ignores_unrelated_kernel_log_lines(self):
        _, text = encode_dump()
        result = parse_dump("unrelated line\n" + text + "\nlast unrelated line")
        self.assertEqual(result["tlv_tag"], 75)

    def test_rejects_missing_or_ambiguous_dump(self):
        _, text = encode_dump()
        with self.assertRaisesRegex(DumpError, "found 0"):
            parse_dump("no diagnostic here")
        with self.assertRaisesRegex(DumpError, "found 2"):
            parse_dump(text + "\n" + text)

    def test_rejects_truncated_or_noncontiguous_rows(self):
        _, text = encode_dump()
        rows = text.splitlines()
        with self.assertRaisesRegex(DumpError, "expected 276 bytes"):
            parse_dump("\n".join(rows[:-1]))
        rows[1] = rows[1].replace("00000010:", "00000011:")
        with self.assertRaisesRegex(DumpError, "expected offset"):
            parse_dump("\n".join(rows))

    def test_rejects_wrong_tag_and_value_length(self):
        _, bad_tag = encode_dump(tag=74)
        with self.assertRaisesRegex(DumpError, "expected resource-config tag"):
            parse_dump(bad_tag)
        _, bad_length = encode_dump(value_length=271)
        with self.assertRaisesRegex(DumpError, "expected TLV value length"):
            parse_dump(bad_length)

    def test_rejects_malformed_bytes_and_unstarted_offset(self):
        _, text = encode_dump()
        rows = text.splitlines()
        rows[0] = rows[0].replace("4b 00", "4b gg")
        with self.assertRaisesRegex(DumpError, "malformed hex-byte"):
            parse_dump("\n".join(rows))
        with self.assertRaisesRegex(DumpError, "starts at nonzero offset"):
            parse_dump("[  2.500000] SUNFISH_WMI_INIT_RSRC 00000010: 00")


if __name__ == "__main__":
    unittest.main()
