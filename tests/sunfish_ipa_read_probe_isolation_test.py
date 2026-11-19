import re
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SYSTEM = ROOT / "platforms/sunfish/ipa-read-probe.system"


class IpaReadProbeIsolationTests(unittest.TestCase):
    def test_exact_ipa_window_has_one_read_only_device_mapping(self):
        system = ET.parse(SYSTEM).getroot()
        regions = {node.get("name"): node for node in system.findall("memory_region")}
        self.assertEqual(
            (regions["ipa_mmio"].get("phys_addr"), regions["ipa_mmio"].get("size")),
            ("0x01e00000", "0x34000"),
        )
        ipa_maps = [m for m in system.findall(".//map") if m.get("mr") == "ipa_mmio"]
        self.assertEqual(len(ipa_maps), 1)
        self.assertEqual(ipa_maps[0].get("perms"), "r")
        self.assertEqual(ipa_maps[0].get("cached"), "false")
        self.assertEqual(ipa_maps[0].get("vaddr"), "0x40000000")

    def test_profile_has_no_interrupt_smc_dma_or_wlan_msa_authority(self):
        system = ET.parse(SYSTEM).getroot()
        pd = system.find("protection_domain")
        self.assertEqual(pd.get("name"), "ipa_read_probe")
        self.assertIsNone(pd.get("smc"))
        self.assertEqual(pd.findall("irq"), [])
        names = {node.get("name") for node in system.findall("memory_region")}
        self.assertEqual(names, {"ipa_mmio", "splash"})
        self.assertEqual(system.findall("channel"), [])

    def test_snapshot_gsi_register_is_inside_the_single_mapped_window(self):
        gsi_offsets = (0x4000 + 0x12000, 0x4000 + 0x1F040)
        self.assertTrue(all(offset + 4 < 0x34000 for offset in gsi_offsets))
        source = (ROOT / "rust/src/sunfish_ipa_read_probe.rs").read_text()
        self.assertIn("const GSI_EE0_STATUS: usize = GSI_BASE_OFFSET + 0x12000;", source)
        self.assertIn("const GSI_STATUS_ENABLED_MASK: u32 = 0x0000_0001;", source)
        self.assertIn("const GSI_V2_2_AP_HW_PARAM_2: usize = GSI_BASE_OFFSET + 0x1f040;", source)
        self.assertIn("read_volatile()", source)
        self.assertNotIn("write_volatile", source)

    def test_ipa_offsets_match_the_pinned_v4_register_profile(self):
        source = (ROOT / "rust/src/sunfish_ipa_read_probe.rs").read_text()
        expected = {
            "IPA_VERSION": "0x034",
            "IPA_COMP_CFG": "0x03c",
            "IPA_CLKON_CFG": "0x044",
            "IPA_SHARED_MEM_SIZE": "0x054",
            "IPA_QSB_MAX_WRITES": "0x074",
            "IPA_QSB_MAX_READS": "0x078",
            "IPA_FILT_ROUT_HASH_EN": "0x148",
            "IPA_BCR": "0x1d0",
            "IPA_COUNTER_CFG": "0x1f0",
            "IPA_TX_CFG": "0x1fc",
        }
        for name, offset in expected.items():
            with self.subTest(register=name):
                self.assertIn(f"const {name}: usize = {offset};", source)
        self.assertIn("const IPA_BYTES: usize = 0x34000;", source)
        offsets_block = source.split("const OFFSETS:", 1)[1].split("];", 1)[0]
        actual_order = re.findall(
            r"\b(IPA_[A-Z_]+|GSI_EE0_STATUS|GSI_V2_2_AP_HW_PARAM_2)\b", offsets_block
        )
        self.assertEqual(
            actual_order,
            [
                "IPA_VERSION",
                "IPA_SHARED_MEM_SIZE",
                "IPA_TX_CFG",
                "IPA_COMP_CFG",
                "IPA_CLKON_CFG",
                "IPA_QSB_MAX_WRITES",
                "IPA_QSB_MAX_READS",
                "IPA_COUNTER_CFG",
                "IPA_BCR",
                "IPA_FILT_ROUT_HASH_EN",
                "GSI_EE0_STATUS",
                "GSI_V2_2_AP_HW_PARAM_2",
            ],
        )
        snapshot_body = source.split("fn snapshot", 1)[1].split(
            "fn render_version_zero", 1
        )[0]
        self.assertLess(
            snapshot_body.index("if version == 0"),
            snapshot_body.index("zip(OFFSETS).skip(1)"),
        )

    def test_each_mmio_read_displays_the_offset_before_access(self):
        source = (ROOT / "rust/src/sunfish_ipa_read_probe.rs").read_text()
        snapshot_body = source.split("fn snapshot", 1)[1].split(
            "fn render_version_zero", 1
        )[0]
        self.assertLess(
            snapshot_body.index("render_read_checkpoint(display, sample, IPA_VERSION)"),
            snapshot_body.index("let version = read32(base, IPA_VERSION)"),
        )
        self.assertLess(
            snapshot_body.index("render_read_checkpoint(display, sample, offset)"),
            snapshot_body.index("*value = read32(base, offset)"),
        )

    def test_host_snapshot_decoder_has_no_mmio_authority(self):
        source = (ROOT / "rust/src/sunfish_ipa_snapshot.rs").read_text()
        for declaration in (
            "pub const IPA_SHARED_MEM_SIZE_OFFSET: u32 = 0x054;",
            "pub const IPA_SHARED_MEM_BADDR_MASK: u32 = 0xffff_0000;",
            "pub const IPA_SHARED_MEM_BADDR_SHIFT: u32 = 16;",
            "pub const IPA_SHARED_MEM_SIZE_MASK: u32 = 0x0000_ffff;",
            "pub const IPA_SHARED_MEM_SIZE_SHIFT: u32 = 0;",
        ):
            with self.subTest(declaration=declaration):
                self.assertIn(declaration, source)
        self.assertIn("decode_shared_mem_size_register", source)
        self.assertIn("register_value & IPA_SHARED_MEM_BADDR_MASK", source)
        self.assertIn("register_value & IPA_SHARED_MEM_SIZE_MASK", source)
        self.assertNotIn("read_volatile", source)
        self.assertNotIn("write_volatile", source)


if __name__ == "__main__":
    unittest.main()
