#!/usr/bin/env python3
"""Check the bounded WCN3990 RX path against pinned Sunfish ath10k source."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "fd78d17954a86ea6ceb0c535e618ea129e09f8dd"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-linux-mainline-ref"
AUDITED_PATHS = (
    "drivers/net/wireless/ath/ath10k/htt.c",
    "drivers/net/wireless/ath/ath10k/htt.h",
    "drivers/net/wireless/ath/ath10k/htt_rx.c",
    "drivers/net/wireless/ath/ath10k/htt_tx.c",
    "drivers/net/wireless/ath/ath10k/core.c",
    "drivers/net/wireless/ath/ath10k/mac.c",
    "drivers/net/wireless/ath/ath10k/htc.h",
    "drivers/net/wireless/ath/ath10k/wmi.c",
    "drivers/net/wireless/ath/ath10k/wmi.h",
    "drivers/net/wireless/ath/ath10k/wmi-tlv.h",
    "drivers/net/wireless/ath/ath10k/rx_desc.h",
    "include/linux/ieee80211.h",
    "include/linux/ieee80211-ht.h",
    "net/wireless/util.c",
)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def section(source: str, start: str, end: str) -> str:
    begin = source.index(start)
    finish = source.index(end, begin)
    return source[begin:finish]


def c_struct(source: str, declaration: str) -> str:
    begin = source.index(declaration)
    brace = source.index("{", begin)
    depth = 0
    for index in range(brace, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                end = source.index(";", index)
                return source[begin : end + 1]
    raise SystemExit(f"FAIL: unterminated C declaration: {declaration}")


def verify_compiled_layout(htt_h: str, rx_desc: str, rx_desc_path: Path) -> None:
    compiler = shutil.which("cc")
    require(compiler is not None, "a host C compiler is required for the descriptor layout check")
    header_length = re.search(r"^#define RX_HTT_HDR_STATUS_LEN\s+(\d+)$", htt_h, re.MULTILINE)
    require(header_length is not None, "RX_HTT_HDR_STATUS_LEN definition changed")
    base = c_struct(htt_h, "struct htt_rx_desc {")
    descriptor = c_struct(htt_h, "struct htt_rx_desc_v2 {")
    in_ord_descriptor = c_struct(htt_h, "struct htt_rx_in_ord_msdu_desc_ext {")
    fields = (
        "attention",
        "frag_info",
        "mpdu_start",
        "mpdu_end",
        "msdu_start",
        "msdu_end",
        "ppdu_start",
        "ppdu_end",
        "rx_hdr_status",
        "msdu_payload",
    )
    with tempfile.TemporaryDirectory(prefix="ath10k-rx-layout-") as temp_name:
        temp = Path(temp_name)
        stub = temp / "linux"
        stub.mkdir()
        (stub / "bitops.h").write_text("#define BIT(nr) (1UL << (nr))\n")
        fields_print = "\n".join(
            f'    printf("{field}=%zu\\n", offsetof(struct htt_rx_desc_v2, {field}));'
            for field in fields
        )
        in_ord_fields = ("msdu_paddr", "msdu_len", "fw_desc", "reserved")
        in_ord_fields_print = "\n".join(
            f'    printf("in_ord_{field}=%zu\\n", offsetof(struct htt_rx_in_ord_msdu_desc_ext, {field}));'
            for field in in_ord_fields
        )
        program = temp / "layout.c"
        program.write_text(
            "#include <stddef.h>\n"
            "#include <stdint.h>\n"
            "#include <stdio.h>\n"
            "typedef uint8_t u8; typedef uint16_t u16; typedef uint32_t u32; typedef uint64_t u64;\n"
            "typedef uint16_t __le16; typedef uint32_t __le32; typedef uint64_t __le64;\n"
            "#define __packed __attribute__((packed))\n"
            f'#include "{rx_desc_path.as_posix()}"\n'
            f"#define RX_HTT_HDR_STATUS_LEN {header_length.group(1)}\n"
            f"{base}\n{descriptor}\n{in_ord_descriptor}\n"
            "int main(void) {\n"
            '    printf("size=%zu\\n", sizeof(struct htt_rx_desc_v2));\n'
            '    printf("in_ord_size=%zu\\n", sizeof(struct htt_rx_in_ord_msdu_desc_ext));\n'
            f"{fields_print}\n"
            f"{in_ord_fields_print}\n"
            "    return 0;\n"
            "}\n"
        )
        binary = temp / "layout"
        compile_result = subprocess.run(
            [compiler, "-std=c11", "-Wall", "-Wextra", "-Werror", "-I", str(temp), str(program), "-o", str(binary)],
            check=False,
            capture_output=True,
            text=True,
        )
        require(compile_result.returncode == 0, f"pinned layout compile failed: {compile_result.stderr.strip()}")
        output = subprocess.check_output([str(binary)], text=True)
    actual = dict(line.split("=", 1) for line in output.splitlines())
    expected = {
        "size": "360",
        "attention": "4",
        "frag_info": "8",
        "mpdu_start": "16",
        "msdu_start": "28",
        "msdu_end": "48",
        "mpdu_end": "104",
        "ppdu_start": "108",
        "ppdu_end": "148",
        "rx_hdr_status": "296",
        "msdu_payload": "360",
        "in_ord_size": "12",
        "in_ord_msdu_paddr": "0",
        "in_ord_msdu_len": "8",
        "in_ord_fw_desc": "10",
        "in_ord_reserved": "11",
    }
    require(actual == expected, f"compiled pinned WCN3990 layout changed: {actual}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    if not source.is_dir():
        raise SystemExit(f"FAIL: source tree not found: {source}")

    commit = subprocess.check_output(
        ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
    ).strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")
    for relative in AUDITED_PATHS:
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", relative],
            check=False,
        )
        require(result.returncode == 0, f"audited source has local edits: {relative}")

    ath = source / "drivers/net/wireless/ath/ath10k"
    htt_h = (ath / "htt.h").read_text()
    htt_c = (ath / "htt.c").read_text()
    htt_rx = (ath / "htt_rx.c").read_text()
    htt_tx = (ath / "htt_tx.c").read_text()
    core = (ath / "core.c").read_text()
    mac = (ath / "mac.c").read_text()
    htc_h = (ath / "htc.h").read_text()
    wmi_c = (ath / "wmi.c").read_text()
    wmi_h = (ath / "wmi.h").read_text()
    wmi_tlv_h = (ath / "wmi-tlv.h").read_text()
    rx_desc = (ath / "rx_desc.h").read_text()
    ieee80211_h = (source / "include/linux/ieee80211.h").read_text()
    ieee80211_ht_h = (source / "include/linux/ieee80211-ht.h").read_text()
    ieee80211_util = (source / "net/wireless/util.c").read_text()
    rust_rx = (ROOT / "rust/src/ath10k_htt_rx.rs").read_text()
    rust_ring = (ROOT / "rust/src/ath10k_htt_rx_ring.rs").read_text()
    rust_owner = (ROOT / "rust/src/ath10k_htt_rx_owner.rs").read_text()
    rust_wmi = (ROOT / "rust/src/ath10k_wmi.rs").read_text()
    rust_wmi_tx = (ROOT / "rust/src/ath10k_wmi_tx.rs").read_text()
    rust_ce = (ROOT / "rust/src/ath10k_ce.rs").read_text()

    desc = section(htt_h, "struct htt_rx_desc_v2 {", "};")
    ordered_fields = (
        "struct htt_rx_desc base;",
        "struct rx_attention attention;",
        "struct rx_frag_info frag_info;",
        "struct rx_mpdu_start mpdu_start;",
        "struct rx_msdu_start msdu_start;",
        "struct rx_msdu_end msdu_end;",
        "u8 rx_hdr_status[RX_HTT_HDR_STATUS_LEN];",
        "u8 msdu_payload[];",
    )
    cursor = 0
    for field in ordered_fields:
        index = desc.find(field, cursor)
        require(index >= 0, f"WCN3990 RX descriptor is missing/reorders {field}")
        cursor = index + len(field)

    wcn_ops = section(htt_c, "const struct ath10k_htt_rx_desc_ops wcn3990_rx_desc_ops", "};")
    require(".rx_desc_size = sizeof(struct htt_rx_desc_v2)" in wcn_ops,
            "WCN3990 descriptor size no longer comes from htt_rx_desc_v2")
    require(".rx_desc_msdu_payload_offset = offsetof(struct htt_rx_desc_v2, msdu_payload)" in wcn_ops,
            "WCN3990 payload offset no longer comes from msdu_payload")
    require(".rx_desc_get_l3_pad_bytes" not in wcn_ops,
            "WCN3990 unexpectedly acquired a nonzero L3-padding getter")
    require("#define HTT_RX_BUF_SIZE 2048" in htt_h, "HTT receive buffer size changed")
    for marker in (
        "#define HTT_RX_RING_SIZE HTT_RX_RING_SIZE_MAX",
        "#define HTT_RX_RING_FILL_LEVEL_DUAL_MAC (HTT_RX_RING_SIZE - 1)",
        "struct htt_rx_ring_setup_ring64 {",
        "__le64 fw_idx_shadow_reg_paddr;",
        "__le64 rx_ring_base_paddr;",
    ):
        require(marker in htt_h, f"pinned WCN3990 RX ring definition changed: {marker}")

    wcn_hw = section(core, ".id = WCN3990_HW_1_0_DEV_VERSION", "\n\t},")
    require(".target_64bit = true" in wcn_hw, "WCN3990 no longer selects 64-bit target addresses")
    require(".decap_align_bytes = 1" in wcn_hw,
            "WCN3990 crypto-parameter alignment changed")
    require(".rx_ring_fill_level = HTT_RX_RING_FILL_LEVEL_DUAL_MAC" in wcn_hw,
            "WCN3990 initial RX fill level changed")
    rx_cfg_64 = section(htt_tx, "static int ath10k_htt_send_rx_ring_cfg_64(", "static int ath10k_htt_send_rx_ring_cfg_hl")
    for marker in (
        "HTT_H2T_MSG_TYPE_RX_RING_CFG",
        "cmd->rx_setup_64.hdr.num_rings = 1",
        "HTT_RX_RING_FLAGS_PHY_DATA_RX",
        "ring->fw_idx_shadow_reg_paddr = __cpu_to_le64(htt->rx_ring.alloc_idx.paddr)",
        "ring->rx_ring_base_paddr = __cpu_to_le64(htt->rx_ring.base_paddr)",
        "ring->rx_ring_len = __cpu_to_le16(htt->rx_ring.size)",
        "ring->rx_ring_bufsize = __cpu_to_le16(HTT_RX_BUF_SIZE)",
        "ring->fw_idx_init_val = __cpu_to_le16(fw_idx)",
    ):
        require(marker in rx_cfg_64, f"ath10k 64-bit RX ring config changed: {marker}")
    rx_init = section(htt_rx, "int ath10k_htt_rx_alloc(", "static int ath10k_htt_rx_crypto_param_len")
    for marker in (
        "htt->rx_ring.size = HTT_RX_RING_SIZE",
        "htt->rx_ring.fill_level = ar->hw_params.rx_ring_fill_level",
        "sizeof(*htt->rx_ring.alloc_idx.vaddr)",
        "htt->rx_ring.sw_rd_idx.msdu_payld = 0",
    ):
        require(marker in rx_init, f"ath10k RX ring initialization changed: {marker}")
    fill = section(htt_rx, "static int __ath10k_htt_rx_ring_fill_n(", "static int ath10k_htt_rx_ring_fill_n")
    require("dev_alloc_skb(HTT_RX_BUF_SIZE + HTT_RX_DESC_ALIGN)" in fill,
            "ath10k RX buffer allocation geometry changed")
    require(fill.index("ath10k_htt_set_paddrs_ring(htt, paddr, idx)") < fill.index("*htt->rx_ring.alloc_idx.vaddr = __cpu_to_le32(idx)"),
            "RX ring producer index is published before the buffer address")
    require("mb();" in fill, "ath10k no longer orders buffer writes before index publication")

    fifo_pop = section(
        htt_rx,
        "static inline struct sk_buff *ath10k_htt_rx_netbuf_pop(",
        "static int ath10k_htt_rx_amsdu_pop(",
    )
    for marker in (
        "htt->rx_ring.netbufs_ring[idx] = NULL",
        "ath10k_htt_reset_paddrs_ring(htt, idx)",
        "htt->rx_ring.sw_rd_idx.msdu_payld = idx",
        "htt->rx_ring.fill_cnt--",
    ):
        require(marker in fifo_pop, f"FIFO RX reclaim changed: {marker}")
    require(
        fifo_pop.index("htt->rx_ring.netbufs_ring[idx] = NULL")
        < fifo_pop.index("ath10k_htt_reset_paddrs_ring(htt, idx)")
        < fifo_pop.index("htt->rx_ring.sw_rd_idx.msdu_payld = idx"),
        "FIFO RX reclaim no longer clears the slot before advancing its read index",
    )

    full_reorder_pop = section(
        htt_rx,
        "static struct sk_buff *ath10k_htt_rx_pop_paddr(",
        "static inline void ath10k_htt_append_frag_list(",
    )
    for marker in ("ath10k_htt_rx_find_skb_paddr(ar, paddr)", "hash_del(&rxcb->hlist)", "htt->rx_ring.fill_cnt--"):
        require(marker in full_reorder_pop, f"full-reorder RX reclaim changed: {marker}")
    require("netbufs_ring" not in full_reorder_pop and "ath10k_htt_reset_paddrs_ring" not in full_reorder_pop,
            "full-reorder RX reclaim unexpectedly became a FIFO slot reclaim")
    owner_markers = (
        "pub enum CompletionMode",
        "CompletionMode::FullReorder",
        "BufferState::DeviceFullReorder",
        "if self.mode != CompletionMode::Fifo",
        "if self.mode != CompletionMode::FullReorder",
        "ring_slot: None",
    )
    for marker in owner_markers:
        require(marker in rust_owner, f"RX owner no longer models completion modes: {marker}")
    require(
        "WMI_TLV_SERVICE_RX_FULL_REORDER_INDEX: usize = 65" in rust_wmi
        and "pub fn rx_full_reorder_enabled(&self) -> Option<bool>" in rust_wmi
        and "pub fn new_from_service_ready(" in rust_owner,
        "RX completion mode is not selected from the source-matched WMI service bit",
    )

    in_ord_enum = section(htt_h, "enum htt_tlv_t2h_msg_type {", "};")
    require(re.search(r"HTT_TLV_T2H_MSG_TYPE_RX_IN_ORD_PADDR_IND\s*=\s*0x12", in_ord_enum) is not None,
            "TLV in-order RX message ID changed")
    in_ord_map = section(htt_c, "static const enum htt_t2h_msg_type htt_tlv_t2h_msg_types[] = {", "};")
    require(re.search(r"\[HTT_TLV_T2H_MSG_TYPE_RX_IN_ORD_PADDR_IND\]\s*=\s*HTT_T2H_MSG_TYPE_RX_IN_ORD_PADDR_IND", in_ord_map) is not None,
            "TLV in-order RX message is no longer mapped by ath10k")
    in_ord_desc = section(htt_h, "struct htt_rx_in_ord_msdu_desc_ext {", "};")
    ordered_in_ord_fields = ("__le64 msdu_paddr;", "__le16 msdu_len;", "u8 fw_desc;", "u8 reserved;")
    cursor = 0
    for field in ordered_in_ord_fields:
        index = in_ord_desc.find(field, cursor)
        require(index >= 0, f"64-bit in-order descriptor is missing/reorders {field}")
        cursor = index + len(field)
    in_ord_ind = section(htt_h, "struct htt_rx_in_ord_ind {", "};")
    ordered_in_ord_header = ("u8 info;", "__le16 peer_id;", "u8 vdev_id;", "u8 reserved;", "__le16 msdu_count;")
    cursor = 0
    for field in ordered_in_ord_header:
        index = in_ord_ind.find(field, cursor)
        require(index >= 0, f"in-order indication header is missing/reorders {field}")
        cursor = index + len(field)
    require("struct htt_rx_in_ord_msdu_desc_ext" in in_ord_ind and "msdu_descs64" in in_ord_ind,
            "in-order indication no longer exposes the 64-bit descriptor array")
    require(re.search(r"SVCMAP\(WMI_TLV_SERVICE_RX_FULL_REORDER,\s*WMI_SERVICE_RX_FULL_REORDER, len\);", wmi_tlv_h) is not None,
            "WMI TLV full-reorder service mapping changed")
    tlv_service_enum = section(wmi_tlv_h, "enum wmi_tlv_service {", "WMI_TLV_MAX_SERVICE = 128,")
    tlv_services = re.findall(
        r"\b(WMI_TLV_SERVICE_[A-Z0-9_]+)\s*(?:=\s*(\d+))?\s*,", tlv_service_enum
    )
    service_index = 0
    full_reorder_index = None
    for name, explicit_index in tlv_services:
        if explicit_index:
            service_index = int(explicit_index)
        if name == "WMI_TLV_SERVICE_RX_FULL_REORDER":
            full_reorder_index = service_index
            break
        service_index += 1
    require(full_reorder_index == 65, f"WMI TLV full-reorder bit moved: {full_reorder_index}")
    require("ar->htt.rx_ring.in_ord_rx = !!(test_bit(WMI_SERVICE_RX_FULL_REORDER," in core,
            "ath10k no longer selects in-order RX from the advertised service")
    in_ord_dispatch = section(htt_rx, "case HTT_T2H_MSG_TYPE_RX_IN_ORD_PADDR_IND:", "case HTT_T2H_MSG_TYPE_TX_CREDIT_UPDATE_IND:")
    require("rx_in_ord_compl_q" in in_ord_dispatch,
            "in-order RX indication no longer reaches the bounded completion queue")
    in_ord_handler = section(htt_rx, "static int ath10k_htt_rx_in_ord_ind(", "static void ath10k_htt_rx_tx_fetch_resp_id_confirm")
    for marker in (
        "if (ar->hw_params.target_64bit)",
        "ath10k_htt_rx_pop_paddr64_list",
        "resp->rx_in_ord_ind.msdu_count",
    ):
        require(marker in in_ord_handler, f"WCN3990 in-order receive handling changed: {marker}")

    required_rx_fields = (
        "RX_ATTENTION_FLAGS_MSDU_LENGTH_ERR     = BIT(17)",
        "RX_ATTENTION_FLAGS_MPDU_LENGTH_ERR     = BIT(27)",
        "RX_ATTENTION_FLAGS_MSDU_DONE           = BIT(31)",
        "u8 ring2_more_count;",
        "#define RX_MSDU_START_INFO0_MSDU_LENGTH_MASK    0x00003fff",
        "#define RX_MSDU_START_INFO1_DECAP_FORMAT_MASK   0x00000300",
        "#define RX_MSDU_START_INFO1_DECAP_FORMAT_LSB    8",
        "RX_MSDU_DECAP_RAW = 0,",
        "#define RX_MSDU_END_INFO0_FIRST_MSDU                BIT(14)",
        "#define RX_MSDU_END_INFO0_LAST_MSDU                 BIT(15)",
    )
    for marker in required_rx_fields:
        require(marker in rx_desc, f"pinned RX descriptor definition changed: {marker}")

    nwifi_hdrlen = section(
        htt_rx,
        "static int ath10k_htt_rx_nwifi_hdrlen(",
        "static void ath10k_htt_rx_h_undecap_raw(",
    )
    for marker in (
        "ieee80211_hdrlen(hdr->frame_control)",
        "ATH10K_FW_FEATURE_NO_NWIFI_DECAP_4ADDR_PADDING",
        "len = round_up(len, 4)",
    ):
        require(marker in nwifi_hdrlen, f"Native-WiFi header padding rule changed: {marker}")
    crypto_params = section(
        htt_rx,
        "static int ath10k_htt_rx_crypto_param_len(",
        "#define MICHAEL_MIC_LEN",
    )
    for marker in (
        "case HTT_RX_MPDU_ENCRYPT_NONE:",
        "return IEEE80211_WEP_IV_LEN;",
        "return IEEE80211_TKIP_IV_LEN;",
        "return IEEE80211_CCMP_HDR_LEN;",
        "return IEEE80211_CCMP_256_HDR_LEN;",
        "return IEEE80211_GCMP_HDR_LEN;",
    ):
        require(marker in crypto_params, f"ath10k crypto-parameter mapping changed: {marker}")
    nwifi_undecap = section(
        htt_rx,
        "static void ath10k_htt_rx_h_undecap_nwifi(",
        "static void *ath10k_htt_rx_h_find_rfc1042(",
    )
    for marker in (
        "const u8 first_hdr[64]",
        "ath10k_htt_rx_desc_get_l3_pad_bytes",
        "ath10k_htt_rx_nwifi_hdrlen(ar, hdr)",
        "ieee80211_get_DA(hdr)",
        "ieee80211_get_SA(hdr)",
        "if (!(status->flag & RX_FLAG_IV_STRIPPED))",
        "ath10k_htt_rx_crypto_param_len(ar, enctype)",
        "ether_addr_copy(ieee80211_get_DA(hdr), da)",
        "ether_addr_copy(ieee80211_get_SA(hdr), sa)",
    ):
        require(marker in nwifi_undecap, f"Native-WiFi reconstruction changed: {marker}")
    require(
        "ar->wmi.rx_decap_mode = ATH10K_HW_TXRX_NATIVE_WIFI;" in core
        and "ATH10K_HW_TXRX_NATIVE_WIFI);" in mac,
        "ath10k no longer defaults WCN3990 RX and vdev TX to Native Wi-Fi",
    )
    tx_mode = section(
        mac,
        "ath10k_mac_tx_h_get_txmode(",
        "static bool ath10k_tx_h_use_hwcrypto(",
    )
    require(
        "if (ieee80211_is_mgmt(fc))\n\t\treturn ATH10K_HW_TXRX_MGMT;" in tx_mode,
        "ath10k management frames no longer select the management TX mode",
    )
    tx_path_policy = section(
        mac,
        "ath10k_mac_tx_h_get_txpath(",
        "static int ath10k_mac_tx_submit(",
    )
    for marker in (
        "case ATH10K_HW_TXRX_NATIVE_WIFI:",
        "return ATH10K_MAC_TX_HTT;",
        "case ATH10K_HW_TXRX_MGMT:",
        "ATH10K_FW_FEATURE_HAS_WMI_MGMT_TX",
        "WMI_SERVICE_MGMT_TX_WMI",
        "return ATH10K_MAC_TX_WMI_MGMT;",
        "else if (ar->htt.target_version_major >= 3)",
        "return ATH10K_MAC_TX_HTT_MGMT;",
    ):
        require(marker in tx_path_policy, f"ath10k management TX-path policy changed: {marker}")
    wmi_mgmt_tx = section(
        wmi_c,
        "ath10k_wmi_op_gen_mgmt_tx(struct ath10k *ar, struct sk_buff *msdu)",
        "static void ath10k_wmi_event_scan_started(",
    )
    for marker in (
        "if (WARN_ON_ONCE(!ieee80211_is_mgmt(hdr->frame_control)))",
        "len = sizeof(cmd->hdr) + msdu->len;",
        "len = round_up(len, 4);",
        "cmd->hdr.vdev_id = __cpu_to_le32(vdev_id);",
        "cmd->hdr.tx_rate = 0;",
        "cmd->hdr.tx_power = 0;",
        "cmd->hdr.buf_len = __cpu_to_le32(buf_len);",
        "ether_addr_copy(cmd->hdr.peer_macaddr.addr, ieee80211_get_DA(hdr));",
        "memcpy(cmd->buf, msdu->data, msdu->len);",
    ):
        require(marker in wmi_mgmt_tx, f"ath10k WMI management-TX serializer changed: {marker}")
    mgmt_header = c_struct(wmi_h, "struct wmi_mgmt_tx_hdr {")
    ordered_mgmt_fields = (
        "__le32 vdev_id;",
        "struct wmi_mac_addr peer_macaddr;",
        "__le32 tx_rate;",
        "__le32 tx_power;",
        "__le32 buf_len;",
    )
    cursor = 0
    for field in ordered_mgmt_fields:
        index = mgmt_header.find(field, cursor)
        require(index >= 0, f"Linux WMI management-TX header changed: {field}")
        cursor = index + len(field)
    command_enum = section(wmi_h, "enum wmi_10_4_cmd_id {", "};")
    command_values: dict[str, int] = {}
    current_command_value = -1
    for line in command_enum.splitlines():
        match = re.match(r"\s*(WMI_10_4_[A-Z0-9_]+)(?:\s*=\s*([^,]+))?,?\s*$", line)
        if match is None:
            continue
        name, expression = match.groups()
        if expression is None:
            current_command_value += 1
        else:
            expression = expression.strip()
            if expression in command_values:
                current_command_value = command_values[expression]
            elif re.fullmatch(r"0x[0-9a-fA-F]+", expression):
                current_command_value = int(expression, 16)
            else:
                continue
        command_values[name] = current_command_value
        if name == "WMI_10_4_MGMT_TX_CMDID":
            break
    mgmt_tx_command_id = command_values.get("WMI_10_4_MGMT_TX_CMDID")
    require(mgmt_tx_command_id == 0x9031, f"Linux WMI 10.4 management-TX ID changed: {mgmt_tx_command_id!r}")
    require(
        re.search(r"#define IEEE80211_MAX_FRAME_LEN\s+2352\b", ieee80211_h) is not None,
        "Linux maximum 802.11 frame length changed",
    )
    rust_mgmt_tx = (ROOT / "rust/src/ath10k_mgmt_tx.rs").read_text()
    for marker in (
        f"WMI_10_4_MGMT_TX_COMMAND_ID: u32 = 0x{mgmt_tx_command_id:04x}",
        "WMI_MGMT_TX_HEADER_BYTES: usize = 24",
        "MAX_MANAGEMENT_FRAME_BYTES: usize = 2352",
        "command[..4].copy_from_slice(&vdev_id.to_le_bytes());",
        "command[4..10].copy_from_slice(&peer_address);",
        "command[20..24].copy_from_slice(&(frame.len() as u32).to_le_bytes());",
        "command[WMI_MGMT_TX_HEADER_BYTES..WMI_MGMT_TX_HEADER_BYTES + frame.len()]",
        "ProtectedManagementUnsupported",
        "wmi_management_tx_supported(",
    ):
        require(marker in rust_mgmt_tx, f"bounded WMI management-TX encoder changed: {marker}")
    for marker in (
        "pub fn queue_management_tx<",
        "if !queue.is_wmi_control_bound()",
        "if !wmi_management_tx_supported(firmware_feature, wmi_service)",
        "encode_management_tx(vdev_id, frame, &mut command_body)",
        ".queue_transfer(",
        "QueueError::Transport",
    ):
        require(marker in rust_mgmt_tx, f"WMI management TX composition gate changed: {marker}")
    htc_wmi_service = section(
        htc_h,
        "enum ath10k_htc_svc_gid {",
        "enum ath10k_htc_svc_id {",
    )
    require(
        "ATH10K_HTC_SVC_GRP_WMI = 1" in htc_wmi_service
        and "#define SVC(group, idx)" in htc_h
        and re.search(
            r"ATH10K_HTC_SVC_ID_WMI_CONTROL\s*=\s*SVC\(ATH10K_HTC_SVC_GRP_WMI,\s*0\)",
            htc_h,
        ) is not None,
        "pinned WMI_CONTROL service identity changed",
    )
    for marker in (
        "pub const HTC_WMI_CONTROL_SERVICE_ID: u16 = 0x0100",
    ):
        require(marker in rust_ce, f"HTC service ID mapping changed: {marker}")
    for marker in (
        "pub fn new_wmi_control(",
        "endpoint.service_id != HTC_WMI_CONTROL_SERVICE_ID",
        "queue.wmi_control_bound = true;",
    ):
        require(marker in rust_wmi_tx, f"HTC WMI_CONTROL queue binding changed: {marker}")
    tx_path = section(mac, "static int ath10k_mac_tx(", "void ath10k_offchan_tx_purge(")
    for marker in (
        "case ATH10K_HW_TXRX_MGMT:",
        "case ATH10K_HW_TXRX_NATIVE_WIFI:",
        "ath10k_tx_h_nwifi(hw, skb);",
        "ath10k_tx_h_seq_no(vif, skb);",
        "ret = ath10k_mac_tx_submit(ar, txmode, txpath, skb);",
    ):
        require(marker in tx_path, f"ath10k management/Native-WiFi TX policy changed: {marker}")
    for marker in (
        "case RX_MSDU_DECAP_RAW:\n\t\tath10k_htt_rx_h_undecap_raw(",
        "case RX_MSDU_DECAP_NATIVE_WIFI:\n\t\tath10k_htt_rx_h_undecap_nwifi(",
    ):
        require(marker in htt_rx, f"ath10k RX frame-class dispatch changed: {marker}")

    pop = section(htt_rx, "static int ath10k_htt_rx_amsdu_pop(", "static struct sk_buff *ath10k_htt_rx_pop_paddr")
    for marker in (
        "RX_ATTENTION_FLAGS_MSDU_DONE",
        "RX_ATTENTION_FLAGS_MPDU_LENGTH_ERR",
        "RX_ATTENTION_FLAGS_MSDU_LENGTH_ERR",
        "ring2_more_count",
        "Note: Chained buffers do not contain rx descriptor",
        "min(msdu_len, HTT_RX_BUF_SIZE)",
    ):
        require(marker in pop, f"ath10k receive assembly policy changed: {marker}")

    unchain = section(htt_rx, "static void ath10k_htt_rx_h_unchain(", "static bool ath10k_htt_rx_validate_amsdu")
    require("decap != RX_MSDU_DECAP_RAW" in unchain,
            "ath10k no longer restricts unchaining to RAW decapsulation")
    require("skb_queue_len(amsdu) != 1 + rxd_frag_info->ring2_more_count" in unchain,
            "ath10k chained-buffer queue-count rule changed")

    rust_markers = (
        "const RX_DESCRIPTOR_BYTES: usize = 360;",
        "const RX_BUFFER_BYTES: usize = 2048;",
        "const RX_PAYLOAD_OFFSET: usize = 360;",
        "const RX_ATTENTION_OFFSET: usize = 4;",
        "const RX_FRAG_INFO_OFFSET: usize = 8;",
        "const RX_MSDU_START_INFO0_OFFSET: usize = 28;",
        "const RX_MSDU_START_INFO1_OFFSET: usize = 36;",
        "const RX_MSDU_END_INFO0_OFFSET: usize = 64;",
        "const FIRST_MSDU: u32 = 1 << 14;",
        "pub fn unchain_raw_msdu(",
    )
    for marker in rust_markers:
        require(marker in rust_rx, f"Rust WCN3990 decoder constant/API changed: {marker}")

    rust_nwifi = (ROOT / "rust/src/ath10k_nwifi_rx.rs").read_text()
    nwifi_rust_markers = (
        "pub const RX_HEADER_STATUS_BYTES: usize = 64;",
        "pub const WCN3990_CRYPTO_PARAM_ALIGNMENT: usize = 1;",
        "pub enum FourAddressPadding",
        "pub enum CryptoParameters",
        "Self::WepIv => 4",
        "Self::TkipOrAesIv => 8",
        "pub fn rebuild_data_msdu(",
        "pub enum Error",
        "UnexpectedQosInDecappedHeader",
        "CryptoParametersOutOfBounds",
    )
    for marker in nwifi_rust_markers:
        require(marker in rust_nwifi, f"Rust Native-WiFi reconstruction changed: {marker}")

    ieee80211_hdrlen = section(
        ieee80211_util,
        "unsigned int __attribute_const__ ieee80211_hdrlen(",
        "EXPORT_SYMBOL(ieee80211_hdrlen);",
    )
    for marker in (
        "if (ieee80211_is_ext(fc))",
        "hdrlen = 4;",
        "if (ieee80211_is_data(fc))",
        "if (ieee80211_has_a4(fc))",
        "if (ieee80211_is_data_qos(fc))",
        "if (ieee80211_has_order(fc))",
        "if (ieee80211_is_mgmt(fc))",
        "if (ieee80211_is_ctl(fc))",
        "(fc & cpu_to_le16(0x00E0)) == cpu_to_le16(0x00C0)",
        "hdrlen = 10;",
        "hdrlen = 16;",
    ):
        require(marker in ieee80211_hdrlen, f"Linux 802.11 header-length rule changed: {marker}")
    for marker in (
        "pub enum FrameKind",
        "pub enum FrameHeaderError",
        "pub struct FrameView<'a>",
        "pub fn inspect_frame(frame: &[u8])",
        "FC_CONTROL_SHORT_HEADER_MASK",
        "HeaderTruncated",
    ):
        require(marker in rust_nwifi, f"bounded 802.11 frame inspector changed: {marker}")

    management_struct = c_struct(ieee80211_h, "struct ieee80211_mgmt {")
    for marker in (
        "__le64 timestamp;",
        "__le16 beacon_int;",
        "__le16 capab_info;",
    ):
        require(marker in management_struct, f"Linux beacon fixed-field layout changed: {marker}")
    ie_struct = c_struct(ieee80211_h, "struct element {")
    for marker in ("u8 id;", "u8 datalen;", "u8 data[];"):
        require(marker in ie_struct, f"Linux information-element layout changed: {marker}")
    ieee80211_eid = section(ieee80211_h, "enum ieee80211_eid {", "};")
    for name, value in (
        ("WLAN_EID_SSID", 0),
        ("WLAN_EID_SUPP_RATES", 1),
        ("WLAN_EID_DS_PARAMS", 3),
        ("WLAN_EID_RSN", 48),
        ("WLAN_EID_EXT_SUPP_RATES", 50),
        ("WLAN_EID_HT_OPERATION", 61),
    ):
        require(
            re.search(rf"\b{name}\s*=\s*{value}\b", ieee80211_eid) is not None,
            f"Linux information-element ID changed: {name}",
        )
    require(
        re.search(r"#define IEEE80211_MAX_SSID_LEN\s+32\b", ieee80211_h) is not None,
        "Linux maximum SSID length changed",
    )
    ht_operation = c_struct(ieee80211_ht_h, "struct ieee80211_ht_operation {")
    ordered_ht_fields = (
        "u8 primary_chan;",
        "u8 ht_param;",
        "__le16 operation_mode;",
        "__le16 stbc_param;",
        "u8 basic_set[16];",
    )
    cursor = 0
    for field in ordered_ht_fields:
        index = ht_operation.find(field, cursor)
        require(index >= 0, f"Linux HT-operation layout changed: {field}")
        cursor = index + len(field)
    require("} __packed;" in ht_operation, "Linux HT-operation IE is no longer packed")
    for marker in (
        "pub enum InformationElementError",
        "pub struct InformationElement<'a>",
        "pub struct InformationElementIter<'a>",
        "pub enum BeaconParseError",
        "pub struct BeaconProbeResponse<'a>",
        "pub fn parse_beacon_probe_response(",
        "pub enum BssAdvertisementError",
        "pub struct BssAdvertisement<'a>",
        "pub fn parse_bss_advertisement(",
        "HT_OPERATION_BYTES: usize = 22",
        "ConflictingPrimaryChannels",
        "MalformedInformationElements",
        "BEACON_PROBE_RESPONSE_FIXED_BYTES: usize = 12",
    ):
        require(marker in rust_nwifi, f"bounded scan-advertisement parser changed: {marker}")

    rust_session = (ROOT / "rust/src/ath10k_htt_rx_session.rs").read_text()
    nwifi_session_markers = (
        "const WCN3990_RX_HEADER_STATUS_OFFSET: usize = 296;",
        "const WCN3990_RX_HEADER_STATUS_END: usize = 360;",
        "pub fn complete_fifo_native_wifi_data_msdu(",
        "WCN3990_NATIVE_WIFI_DECAP_FORMAT: u8 = 1;",
        "NativeWifiMsduBoundary",
        "descriptor.first_msdu",
        "descriptor.last_msdu",
    )
    for marker in nwifi_session_markers:
        require(marker in rust_session, f"Rust Native-WiFi RX completion composition changed: {marker}")

    probe_tx = (ROOT / "rust/src/ieee80211_probe.rs").read_text()
    for marker in (
        "const IEEE80211_PROBE_REQUEST_FC: u16 = 0x0040;",
        "const IEEE80211_BROADCAST: [u8; 6] = [0xff; 6];",
        "pub struct ProbeRequest<'a>",
        "pub fn encode_probe_request(",
        "request.sequence_number << 4",
        "SUPPORTED_RATES_ELEMENT_ID",
        "EXTENDED_SUPPORTED_RATES_ELEMENT_ID",
    ):
        require(marker in probe_tx, f"bounded Probe Request encoder changed: {marker}")

    ring_markers = (
        "pub const WCN3990_RX_RING_SLOTS: u32 = 2048;",
        "pub const WCN3990_RX_RING_FILL_LEVEL: u32 = WCN3990_RX_RING_SLOTS - 1;",
        "pub const WCN3990_RX_BUFFER_BYTES: u32 = 2048;",
        "pub const WCN3990_RX_DESCRIPTOR_BYTES: u32 = 360;",
        "pub const WCN3990_RX_RING_CONFIG_BYTES: usize = 48;",
        "mac80211_header_words: 74,",
        "msdu_payload_words: 90,",
        "ppdu_start_words: 27,",
        "ppdu_end_words: 37,",
        "mpdu_start_words: 4,",
        "mpdu_end_words: 26,",
        "msdu_start_words: 7,",
        "msdu_end_words: 12,",
        "attention_words: 1,",
        "fragment_info_words: 2,",
    )
    for marker in ring_markers:
        require(marker in rust_ring, f"Rust WCN3990 ring constant/layout changed: {marker}")

    in_ord_rust_markers = (
        "const RX_IN_ORD_T2H_MESSAGE_TYPE: u8 = 0x12;",
        "const RX_IN_ORD_64_DESCRIPTOR_BYTES: usize = 12;",
        "const RX_IN_ORD_MAX_DESCRIPTORS: u16 = 2047;",
        "pub fn parse_in_order_paddr_indication(",
        "pub buffer_iova: u64,",
    )
    for marker in in_ord_rust_markers:
        require(marker in rust_rx, f"Rust WCN3990 in-order RX parser changed: {marker}")

    verify_compiled_layout(htt_h, rx_desc, ath / "rx_desc.h")

    print(f"PASS: pinned source {commit}; audited RX files are unmodified")
    print("PASS: WCN3990 descriptor ordering, RX completion/error bits, and field masks match")
    print("PASS: RX queue assembly and RAW single-MSDU unchaining policy match ath10k")
    print("PASS: bounded Rust decoder retains the reviewed WCN3990 buffer/field offsets")
    print("PASS: Native-WiFi RX padding, header-status, address, and crypto rules match pinned ath10k")
    print("PASS: bounded 802.11 frame-header inspection matches pinned Linux header-length rules")
    print("PASS: BSS parser IE IDs, SSID bound, and packed HT-operation layout match pinned Linux headers")
    print("PASS: Probe Request, WMI management serializer, capability gate, WMI_CONTROL binding, and HTC/CE composition match pinned source policy")
    print("PASS: FIFO lease composition binds header status to its buffer and rejects partial MSDU groups")
    print("PASS: compiled WCN3990 receive descriptor, in-order 64-bit entry, and RX_RING_CFG geometry match")
    print("PASS: TLV full-reorder selection, 64-bit in-order descriptor schema, and bounded parser match")
    print("PASS: WMI full-reorder capability selects the matching ownership mode; FIFO and IOVA reclaim stay distinct")
    print("LIMIT: validates source policy and ring preflight, not radio or firmware compatibility")


if __name__ == "__main__":
    main()
