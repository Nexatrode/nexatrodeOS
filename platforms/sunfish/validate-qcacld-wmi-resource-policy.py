#!/usr/bin/env python3
"""Check Sunfish QCACLD WMI resource inputs and decoder layout against Linux."""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-lineage-kernel"
SERVICE_BITMAP_PATCH = ROOT / "patches/sunfish-wmi-service-bitmap-dump.patch"
WMA_PATH = "drivers/staging/qcacld-3.0/core/wma/src/wma_main.c"
HDD_CONFIG_PATH = "drivers/staging/qcacld-3.0/core/hdd/inc/wlan_hdd_cfg.h"
TARGET_CONFIG_PATH = "drivers/staging/qcacld-3.0/core/mac/inc/wlan_tgt_def_config.h"
SIR_CONFIG_PATH = "drivers/staging/qcacld-3.0/core/mac/inc/sir_api.h"
WMI_API_PATH = "drivers/staging/fw-api/fw/wmi_unified.h"
WMI_SERVICES_PATH = "drivers/staging/fw-api/fw/wmi_services.h"
WMI_UNIFIED_PARAM_PATH = "drivers/staging/qca-wifi-host-cmn/wmi/inc/wmi_unified_param.h"
WMI_TLV_DEFS_PATH = "drivers/staging/fw-api/fw/wmi_tlv_defs.h"
WMI_TLV_HELPER_PATH = "drivers/staging/fw-api/fw/wmi_tlv_helper.h"
WMI_TLV_PATH = "drivers/staging/qca-wifi-host-cmn/wmi/src/wmi_unified_tlv.c"
WMI_CORE_PATH = "drivers/staging/qca-wifi-host-cmn/wmi/src/wmi_unified.c"
WMA_INTERNAL_PATH = "drivers/staging/qcacld-3.0/core/wma/inc/wma_internal.h"
WMA_HEADER_PATH = "drivers/staging/qcacld-3.0/core/wma/inc/wma.h"
CDS_UTILS_PATH = "drivers/staging/qcacld-3.0/core/cds/inc/cds_utils.h"
AUDITED_PATHS = (
    WMA_PATH,
    HDD_CONFIG_PATH,
    TARGET_CONFIG_PATH,
    SIR_CONFIG_PATH,
    WMI_API_PATH,
    WMI_SERVICES_PATH,
    WMI_UNIFIED_PARAM_PATH,
    WMI_TLV_DEFS_PATH,
    WMI_TLV_HELPER_PATH,
    WMI_TLV_PATH,
    WMI_CORE_PATH,
    WMA_INTERNAL_PATH,
    WMA_HEADER_PATH,
    CDS_UTILS_PATH,
)


def git(source: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(source), *args], text=True)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def macro(text: str, name: str) -> str:
    match = re.search(rf"^\s*#define\s+{re.escape(name)}\s+([^/\n]+)", text, re.M)
    require(match is not None, f"missing source macro {name}")
    return " ".join(match.group(1).split())


def normalized(text: str) -> str:
    return " ".join(text.split())


def rust_u32_array(name: str, source: str) -> list[int]:
    match = re.search(
        rf"const\s+{re.escape(name)}\s*:\s*\[u32;\s*\d+\s*\]\s*=\s*\[(.*?)\];",
        source,
        re.S,
    )
    require(match is not None, f"missing captured Rust fixture {name}")
    return [int(value, 16) for value in re.findall(r"0x[0-9a-fA-F]+", match.group(1))]


def enum_value(text: str, enum_name: str, member: str) -> int:
    end = re.search(rf"\}}\s*{re.escape(enum_name)}\s*;", text)
    require(end is not None, f"missing pinned {enum_name} enum")
    start = text.rfind("typedef enum", 0, end.start())
    require(start >= 0, f"missing start of pinned {enum_name} enum")
    body_start = text.find("{", start, end.start())
    require(body_start >= 0, f"missing body of pinned {enum_name} enum")
    body = re.sub(r"/\*.*?\*/|//[^\n]*", "", text[body_start + 1:end.start()], flags=re.S)
    values: dict[str, int] = {}
    previous = -1
    for entry in body.split(","):
        entry = entry.strip()
        if not entry:
            continue
        item = re.fullmatch(r"(\w+)\s*(?:=\s*(.+))?", entry, re.S)
        require(item is not None, f"cannot parse {enum_name} entry: {entry}")
        name, expression = item.groups()
        if expression is None:
            value = previous + 1
        else:
            expression = expression.strip()
            if re.fullmatch(r"0[xX][0-9A-Fa-f]+|\d+", expression):
                value = int(expression, 0)
            elif expression in values:
                value = values[expression]
            else:
                require(False, f"unsupported {enum_name} value: {expression}")
                raise AssertionError("unreachable")
        values[name] = value
        previous = value
    require(member in values, f"missing {enum_name} member {member}")
    return values[member]


def resource_config_fields(header: str) -> tuple[str, ...]:
    end_marker = "} wmi_resource_config;"
    end = header.find(end_marker)
    require(end >= 0, "missing pinned wmi_resource_config declaration")
    start = header.rfind("typedef struct {", 0, end)
    require(start >= 0, "missing start of wmi_resource_config declaration")
    body = header[start:end]
    body = re.sub(r"/\*.*?\*/", "", body, flags=re.S)
    fields: list[str] = []
    for name, count in re.findall(
        r"\bA_UINT32\s+(\w+)\s*(?:\[\s*(\d+)\s*\])?\s*;", body
    ):
        expanded = [f"{name}[{index}]" for index in range(int(count))] if count else [name]
        fields.extend(expanded)
    require(fields and fields[0] == "tlv_header",
            "wmi_resource_config no longer starts with a TLV header")
    return tuple(fields[1:])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    if not source.is_dir():
        raise SystemExit(f"FAIL: source tree not found: {source}")

    commit = git(source, "rev-parse", "HEAD").strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")
    for path in AUDITED_PATHS:
        clean = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", path],
            check=False,
        )
        require(clean.returncode == 0, f"audited source has local edits: {path}")

    patch_text = SERVICE_BITMAP_PATCH.read_text()
    patch_files = re.findall(r"^(?:---|\+\+\+) [ab]/(.+)$", patch_text, re.M)
    require(patch_files == [WMI_TLV_PATH, WMI_TLV_PATH, WMA_PATH, WMA_PATH],
            "service-bitmap diagnostic patch must touch only the active WMA and WMI handlers")
    for marker in (
        "SUNFISH_WMI_SERVICE_READY",
        "SUNFISH_WMI_BASE_SERVICE_BITMAP",
        "SUNFISH_WMI_SERVICE_AVAILABLE",
        "SUNFISH_WMI_EXT_SERVICE_BITMAP",
        "wmi_service_segment_offset",
    ):
        require(marker in patch_text, f"diagnostic patch is missing {marker}")
    patch_check = subprocess.run(
        ["git", "-C", str(source), "apply", "--check", str(SERVICE_BITMAP_PATCH)],
        text=True,
        capture_output=True,
        check=False,
    )
    require(patch_check.returncode == 0,
            f"diagnostic patch no longer applies cleanly: {patch_check.stderr.strip()}")

    (wma, hdd, target, sir, wmi_api, wmi_services, wmi_unified_param,
     wmi_tlv_defs, wmi_tlv_helper, wmi_tlv, wmi_core, wma_internal,
     wma_header, cds_utils) = (
        git(source, "show", f"HEAD:{path}") for path in AUDITED_PATHS
    )
    rust = (ROOT / "rust/src/qcacld_wmi.rs").read_text()
    profile = (ROOT / "rust/src/sunfish_qcacld_resource.rs").read_text()
    resource_test = (ROOT / "tests/sunfish_qcacld_resource_test.rs").read_text()
    decoder_source = (ROOT / "platforms/sunfish/parse-wmi-init-resource-dump.py").read_text()
    decoder_fields_match = re.search(
        r"RESOURCE_FIELD_NAMES\s*=\s*\((?P<names>.*?)\)",
        decoder_source,
        re.S,
    )
    require(decoder_fields_match is not None,
            "decoder no longer declares its ordered resource field names")
    decoder_fields = tuple(re.findall(r'"([^"\\]*(?:\\.[^"\\]*)*)"',
                                      decoder_fields_match.group("names")))
    source_fields = resource_config_fields(wmi_api)
    require(len(source_fields) == 68,
            f"pinned WMI resource config has {len(source_fields)} value words, expected 68")
    require(decoder_fields == source_fields,
            "decoder field names/order no longer match pinned wmi_resource_config")
    require(
        "#define WMITLV_SET_HDR(tlv_buf, tag, len) (((A_UINT32 *)(tlv_buf))[0]) = ((tag << 16) | (len & 0x0000FFFF))"
        in normalized(wmi_tlv_helper),
        "pinned WMI TLV header packing no longer matches the decoder",
    )

    require(macro(target, "CFG_TGT_NUM_VDEV") == "4",
            "the non-high-latency target vdev default changed")
    require(macro(sir, "SIR_SAP_MAX_NUM_PEERS") == "32",
            "the Sunfish max-station upper bound changed")
    require(macro(hdd, "CFG_SAP_MAX_NO_PEERS") == '"gSoftApMaxPeers"',
            "the max-station INI key changed")
    require(macro(hdd, "CFG_SAP_MAX_NO_PEERS_DEFAULT") == "(SIR_SAP_MAX_NUM_PEERS)",
            "the max-station default source changed")
    require(macro(hdd, "CFG_SAP_MAX_OFFLOAD_PEERS") == '"gMaxOffloadPeers"',
            "the offload-peer INI key changed")
    require(macro(hdd, "CFG_SAP_MAX_OFFLOAD_PEERS_DEFAULT") == "(2)",
            "the offload-peer default changed")
    require(macro(hdd, "CFG_SAP_MAX_OFFLOAD_PEERS_MIN") == "(2)"
            and macro(hdd, "CFG_SAP_MAX_OFFLOAD_PEERS_MAX") == "(5)",
            "the offload-peer configuration bounds changed")
    require(macro(hdd, "CFG_SAP_MAX_OFFLOAD_REORDER_BUFFS")
            == '"gMaxOffloadReorderBuffs"',
            "the reorder-buffer INI key changed")
    require(macro(hdd, "CFG_SAP_MAX_OFFLOAD_REORDER_BUFFS_DEFAULT") == "(2)"
            and macro(hdd, "CFG_SAP_MAX_OFFLOAD_REORDER_BUFFS_MIN") == "(0)"
            and macro(hdd, "CFG_SAP_MAX_OFFLOAD_REORDER_BUFFS_MAX") == "(3)",
            "the reorder-buffer default or bounds changed")
    require(macro(hdd, "CFG_MAX_SCAN_COUNT_NAME") == '"max_scan_count"'
            and macro(hdd, "CFG_MAX_SCAN_COUNT_DEFAULT") == "(4)"
            and macro(hdd, "CFG_MAX_SCAN_COUNT_MIN") == "(1)"
            and macro(hdd, "CFG_MAX_SCAN_COUNT_MAX") == "(8)",
            "the scan-count key, default, or bounds changed")

    compact_wma = normalized(wma)
    require("wma_handle->wlan_init_status = ev->status;" in compact_wma,
            "QCACLD no longer stores the WMI READY status from the event")
    require("wma_info(\"FW ready event received\");" in compact_wma,
            "QCACLD READY wait no longer logs successful completion")
    compact_wmi_tlv = normalized(wmi_tlv)
    require('qdf_print("%s:%d\\n", __func__, ev->status);' in compact_wmi_tlv
            and "return ev->status;" in compact_wmi_tlv,
            "QCACLD READY status log no longer reports the event status")
    require("#define MAX_SUPPORTED_PEERS_REV1_1 14" in compact_wma
            and "#define MAX_SUPPORTED_PEERS_REV1_3 32" in compact_wma
            and "case AR6320_REV1_1_VERSION:" in compact_wma,
            "the WMA target-revision peer caps changed")
    require("tgt_cfg->num_peers = no_of_peers_supported + CFG_TGT_NUM_VDEV + 2;"
            in compact_wma,
            "the WMA peer-count formula changed")
    require("return 2 * (no_of_peers_supported + CFG_TGT_NUM_VDEV + 2);"
            in compact_wma,
            "the non-high-latency WMA TID formula changed")
    require("tgt_cfg->scan_max_pending_req = wma_handle->max_scan;" in compact_wma,
            "the WMA scan-pending assignment changed")
    require("wlan_res_cfg->num_offload_peers = cds_cfg->ap_maxoffload_peers + 1;"
            in compact_wma
            and "wlan_res_cfg->num_offload_reorder_buffs = cds_cfg->ap_maxoffload_reorderbuffs + 1;"
            in compact_wma,
            "the WMA offload count adjustments changed")
    require("wlan_res_cfg->num_keep_alive_pattern = WMA_MAXNUM_PERIODIC_TX_PTRNS;"
            in compact_wma,
            "the final keep-alive pattern count assignment changed")
    require("wma_handle->max_scan = cds_cfg->max_scan;" in compact_wma
            and "tgt_cfg->scan_max_pending_req = wma_handle->max_scan;" in compact_wma,
            "the WMI scan count no longer follows the effective host setting")
    for fragment in (
        "cfg->max_frag_entries = QDF_MIN(QCA_OL_11AC_TX_MAX_FRAGS, target_if_get_max_frag_entry(tgt_hdl));",
        "cfg->num_wow_filters = ucfg_pmo_get_num_wow_filters(psoc);",
        "cfg->apf_instruction_size = ucfg_pmo_get_apf_instruction_size(psoc);",
        "cfg->num_packet_filters = ucfg_pmo_get_num_packet_filters(psoc);",
        "wma_update_fw_config(wma_handle->psoc, tgt_hdl);",
    ):
        require(fragment in compact_wma,
                f"pinned runtime WMI resource input changed: {fragment}")
    require("wmi_service_deprecated_replace" in compact_wma
            and "wlan_res_cfg->use_pdev_id = true;" in compact_wma
            and "wlan_res_cfg->use_pdev_id = false;" in compact_wma,
            "the service-ready-dependent PDEV-ID selection changed")
    require("wmi_service_new_htt_msg_format" in compact_wma
            and "wlan_res_cfg->new_htt_msg_format = true;" in compact_wma
            and "wlan_res_cfg->new_htt_msg_format = false;" in compact_wma,
            "the service-ready-dependent HTT framing flag changed")
    require("wmi_service_peer_unmap_cnf_support" in compact_wma
            and "wlan_res_cfg->peer_unmap_conf_support = true;" in compact_wma
            and "wlan_res_cfg->peer_unmap_conf_support = false;" in compact_wma,
            "the host-and-firmware-gated peer-unmap flag changed")

    # Audit the source-derived 68-word candidate against the WMA defaults,
    # WMI serialization, service bitmap positions, and zeroed transport buffer.
    expected_defaults = {
        "CFG_TGT_NUM_PEER_KEYS": "2",
        "CFG_TGT_AST_SKID_LIMIT": "16",
        "CFG_TGT_DEFAULT_TX_CHAIN_MASK": "0x7",
        "CFG_TGT_DEFAULT_RX_CHAIN_MASK": "0x7",
        "CFG_TGT_RX_TIMEOUT_LO_PRI": "100",
        "CFG_TGT_RX_TIMEOUT_HI_PRI": "40",
        "CFG_TGT_RX_DECAP_MODE": "(0x2)",
        "CFG_TGT_DEFAULT_BMISS_OFFLOAD_MAX_VDEV": "0x3",
        "CFG_TGT_DEFAULT_ROAM_OFFLOAD_MAX_VDEV": "0x3",
        "CFG_TGT_DEFAULT_ROAM_OFFLOAD_MAX_PROFILES": "0x8",
        "CFG_TGT_DEFAULT_TX_DBG_LOG_SIZE": "1024",
        "CFG_TGT_DEFAULT_VOW_CONFIG": "0",
        "CFG_TGT_MAX_FRAG_TABLE_ENTRIES": "10",
        "CFG_TGT_MAX_MULTICAST_FILTER_ENTRIES": "32",
        "CFG_TGT_NUM_TDLS_VDEVS": "1",
        "CFG_TGT_NUM_TDLS_CONC_SLEEP_STAS": "1",
        "CFG_TGT_NUM_TDLS_CONC_BUFFER_STAS": "1",
        "CFG_TGT_NUM_OCB_VDEVS": "1",
        "CFG_TGT_NUM_OCB_CHANNELS": "2",
        "CFG_TGT_NUM_OCB_SCHEDULES": "2",
    }
    for name, expected in expected_defaults.items():
        require(macro(target, name) == expected,
                f"pinned target default {name} changed")
    require(macro(wma_internal, "WMA_MAXNUM_PERIODIC_TX_PTRNS") == "6",
            "the final keep-alive pattern count changed")
    require(macro(cds_utils, "CDS_DBS_SCAN_CLIENTS_MAX") == "(7)",
            "the DBS scan duty-cycle count changed")
    require(macro(wma_header, "QCA_OL_11AC_TX_MAX_FRAGS") == "2",
            "the final WMA fragment-entry cap changed")
    require(macro(hdd, "CFG_ENABLE_PEER_UNMAP_CONF_DEFAULT") == "(0)",
            "the host peer-unmap confirmation default changed")
    for name, bit in (
        ("WMI_SERVICE_DEPRECATED_REPLACE", 99),
        ("WMI_SERVICE_HTT_H2T_NO_HTC_HDR_LEN_IN_MSG_LEN", 176),
        ("WMI_SERVICE_PEER_UNMAP_RESPONSE_SUPPORT", 182),
    ):
        require(re.search(rf"\b{name}\s*=\s*{bit}\b", wmi_services) is not None,
                f"pinned WMI service index changed: {name}")

    compact_services = normalized(wmi_services.replace("\\\n", " "))
    require(re.search(r"\bWMI_MAX_SERVICE\s*=\s*128\b", wmi_services) is not None,
            "the base WMI service bitmap boundary changed")
    require("WMI_SERVICE_BM_SIZE ((WMI_MAX_SERVICE + sizeof(A_UINT32)- 1)/sizeof(A_UINT32))"
            in compact_services,
            "the base WMI service bitmap sizing formula changed")
    require("(pwmi_svc_bmap)[(svc_id)/(sizeof(A_UINT32))]" in compact_services
            and "(svc_id)%(sizeof(A_UINT32))" in compact_services,
            "QCACLD base-service byte-stride indexing changed")
    require("((svc_id) - WMI_MAX_SERVICE) / 32" in compact_services
            and "((svc_id) & 0x1f)" in compact_services,
            "QCACLD extended-service bitmap indexing changed")
    require(macro(wmi_api, "WMI_SERVICE_SEGMENT_BM_SIZE32") == "4",
            "SERVICE_AVAILABLE segment is no longer four words")
    service_available_struct = re.search(
        r"typedef\s+struct\s*\{(?P<body>.*?)\}\s*wmi_service_available_event_fixed_param\s*;",
        wmi_api,
        re.S,
    )
    require(service_available_struct is not None
            and re.search(r"A_UINT32\s+wmi_service_segment_offset\s*;",
                          service_available_struct.group("body")) is not None
            and re.search(r"A_UINT32\s+wmi_service_segment_bitmap\s*\[\s*WMI_SERVICE_SEGMENT_BM_SIZE32\s*\]",
                          service_available_struct.group("body")) is not None,
            "SERVICE_AVAILABLE fixed-event layout changed")
    require(enum_value(wmi_tlv_defs, "WMITLV_TAG_ID",
                       "WMITLV_TAG_STRUC_wmi_service_available_event_fixed_param") == 559,
            "SERVICE_AVAILABLE TLV tag changed from the Rust parser")
    require("wmi_service[wmi_service_new_htt_msg_format] = WMI_SERVICE_HTT_H2T_NO_HTC_HDR_LEN_IN_MSG_LEN;"
            in normalized(wmi_tlv)
            and "wmi_service[wmi_service_peer_unmap_cnf_support] = WMI_SERVICE_PEER_UNMAP_RESPONSE_SUPPORT;"
            in normalized(wmi_tlv),
            "the host service-name to wire-ID mapping changed")

    compact_tlv = normalized(wmi_tlv)
    for fragment in (
        "resource_cfg->num_vdevs = tgt_res_cfg->num_vdevs;",
        "resource_cfg->num_peers = tgt_res_cfg->num_peers;",
        "resource_cfg->num_msdu_desc = tgt_res_cfg->num_msdu_desc;",
        "resource_cfg->max_frag_entries = tgt_res_cfg->max_frag_entries;",
        "resource_cfg->num_wow_filters = tgt_res_cfg->num_wow_filters;",
        "resource_cfg->bpf_instruction_size = tgt_res_cfg->apf_instruction_size;",
        "resource_cfg->num_packet_filters = tgt_res_cfg->num_packet_filters;",
        "resource_cfg->use_pdev_id = tgt_res_cfg->use_pdev_id;",
        "resource_cfg->max_num_dbs_scan_duty_cycle = tgt_res_cfg->max_num_dbs_scan_duty_cycle;",
        "WMI_RSRC_CFG_FLAG_MGMT_COMP_EVT_BUNDLE_SUPPORT_SET(",
        "WMI_RSRC_CFG_FLAG_TX_MSDU_ID_NEW_PARTITION_SUPPORT_SET(",
    ):
        require(fragment in compact_tlv,
                f"pinned WMI resource serializer changed: {fragment}")
    require("WMI_RSRC_CFG_FLAG_HTT_H2T_NO_HTC_HDR_LEN_IN_MSG_LEN_SET(" in wmi_tlv
            and "WMI_RSRC_CFG_FLAG_PEER_UNMAP_RESPONSE_SUPPORT_SET(" in wmi_tlv,
            "dynamic WMI resource flag serialization changed")
    require("OS_MEMZERO(qdf_nbuf_data(wmi_buf), len);" in wmi_core,
            "WMI buffers are no longer cleared before resource serialization")
    require("const SERVICE_DEPRECATED_REPLACE: usize = 99;" in profile
            and "const SERVICE_HTT_H2T_NO_HTC_HDR_LEN_IN_MSG_LEN: usize = 176;" in profile
            and "const SERVICE_PEER_UNMAP_RESPONSE_SUPPORT: usize = 182;" in profile,
            "Rust candidate service gates diverge from pinned service indices")
    compact_profile_for_services = normalized(profile)
    require("service_ready .service_enabled(service_index)" in compact_profile_for_services
            and "service_available .service_enabled(service_index)" in compact_profile_for_services
            and "const SERVICE_BASE_INDEX_STRIDE: usize = core::mem::size_of::<u32>();" in rust
            and "pub fn service_enabled(&self, service_index: usize) -> Option<bool>" in rust,
            "Rust candidate no longer separates base and extended service bitmaps")
    require("let mut words = [0u32; RESOURCE_CONFIG_WORDS];" in profile
            and "FLAG_MGMT_COMPLETION_BUNDLE | FLAG_TX_MSDU_NEW_PARTITION" in profile
            and "&& inputs.enable_peer_unmap_confirmation" in profile
            and "if inputs.compile.high_latency_hif" in profile
            and "UnsupportedHighLatencyBuild" in profile,
            "Rust candidate no longer starts from cleared words or host-gates peer-unmap")
    require("const FLAG_MGMT_COMPLETION_BUNDLE: u32 = 1 << 9;" in profile
            and "const FLAG_TX_MSDU_NEW_PARTITION: u32 = 1 << 10;" in profile
            and "const FLAG_NEW_HTT_MESSAGE_FORMAT: u32 = 1 << 19;" in profile
            and "const FLAG_PEER_UNMAP_RESPONSE: u32 = 1 << 20;" in profile,
            "Rust WMI INIT flag positions changed")
    require("observed_lineage_wmi_init_2026_10_24" in profile
            and "words[29] = effective.max_frag_entries;" in profile
            and "words[34] = effective.num_wow_filters;" in profile
            and "words[51] = effective.apf_instruction_size;" in profile
            and "words[61] = effective.num_packet_filters;" in profile,
            "Rust candidate no longer applies the captured effective resource overrides")
    compact_profile = normalized(profile)
    require("startup .encode_init_body(&resource_tlv, hardware_mode_index, wmi_body_output)"
            in compact_profile
            and "qcom_htc::encode_wmi_command_frame(" in compact_profile
            and "SUNFISH_WMI_INIT_COMMAND_ID" in compact_profile,
            "Rust candidate no longer composes through the QCACLD INIT and HTC encoders")

    captured_base_words = rust_u32_array(
        "CAPTURED_LINEAGE_BASE_SERVICE_WORDS", resource_test
    )
    captured_extended_words = rust_u32_array(
        "CAPTURED_LINEAGE_EXT_SERVICE_WORDS", resource_test
    )
    require(captured_base_words == [
        0x0000000d, 0x00000007, 0x0000000f, 0x00000003,
        0x0000000f, 0x00000009, 0x00000009, 0x0000000f,
        0x0000000a, 0x00000009, 0x00000002, 0x0000000a,
        0x00000001, 0x0000000d, 0x00000007, 0x0000000f,
        0x0000000b, 0x00000004, 0x00000004, 0x0000000b,
        0x00000007, 0x00000000, 0x00000008, 0x00000001,
        0x00000009, 0x00000003, 0x0000000d, 0x00000001,
        0x00000005, 0x0000000e, 0x00000008, 0x00000005,
    ], "captured base service words no longer match the device log")
    require(captured_extended_words == [
        0x4531e0c8, 0x00512000, 0x00002000, 0x00000000,
    ], "captured extended service words no longer match the device log")
    compact_resource_test = normalized(resource_test)
    require(
        compact_resource_test.count(
            "service_ready_words_fixture(CAPTURED_LINEAGE_BASE_SERVICE_WORDS)"
        ) >= 2
        and compact_resource_test.count(
            "service_available_words_fixture(CAPTURED_LINEAGE_EXT_SERVICE_WORDS)"
        ) >= 2,
        "the observed-profile and frame-composition tests must use live service bitmaps",
    )

    require("const QCACLD_NUM_VDEVS: u8 = 4;" in rust
            and "const QCACLD_MAX_MAX_STATION: u8 = 32;" in rust
            and "const QCACLD_MIN_OFFLOAD_PEERS: u8 = 2;" in rust
            and "const QCACLD_MAX_OFFLOAD_PEERS: u8 = 5;" in rust
            and "const QCACLD_MAX_OFFLOAD_REORDER_BUFFERS: u8 = 3;" in rust
            and "const QCACLD_MIN_SCAN_COUNT: u8 = 1;" in rust
            and "const QCACLD_MAX_SCAN_COUNT: u8 = 8;" in rust,
            "the Rust WMA count helper no longer matches pinned source defaults")
    require("num_tids: peers_plus_vdevs * 2," in rust
            and "num_offload_peers: self.ap_max_offload_peers + 1," in rust
            and "num_offload_reorder_buffers: self.ap_max_offload_reorder_buffers + 1," in rust,
            "the Rust WMA count derivation changed")

    print(f"PASS: pinned, clean Lineage QCACLD source {commit}")
    print("PASS: 68-word WMI INIT decoder field order matches pinned QCACLD ABI")
    print("PASS: WMI TLV header byte order matches pinned WMITLV_SET_HDR")
    print("PASS: Sunfish WMA defaults, configuration bounds, and target caps match the helper")
    print("PASS: host, service-ready, and runtime PMO inputs to WMI resource fields match the pinned path")
    print("PASS: full 68-word candidate defaults, WMI service gates, and cleared-buffer behavior match pinned source")
    print("PASS: base and extended WMI service bitmap semantics match the pinned source; diagnostic patch applies cleanly")
    print("PASS: captured base/extended service words drive the live-profile and INIT-frame regressions")
    print("PASS: pinned QCACLD stores and logs WMI READY event status; stock Lineage boot log records status 0 and successful READY wait")
    print("LIMIT: raw SERVICE_READY/READY event bytes and a seL4 transport submission remain untested")


if __name__ == "__main__":
    main()
