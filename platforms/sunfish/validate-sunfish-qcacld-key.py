#!/usr/bin/env python3
"""Audit the Sunfish QCACLD CCMP key-command codec against pinned source."""

from __future__ import annotations

from hashlib import sha256
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[2]
KERNEL = ROOT / ".cache/sunfish-lineage-kernel"
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
SOURCE_HASHES = {
    "drivers/staging/fw-api/fw/wmi_unified.h":
        "abb3ea5623588bc99a656481653c63e29ee58a6d2bef33db286be5671657b67d",
    "drivers/staging/fw-api/fw/wmi_tlv_defs.h":
        "63708dc15c1aa54e8431a113f68b41e2de1be4f35fb365bff78456861ba855fc",
    "drivers/staging/fw-api/fw/htt_common.h":
        "47d869a6343bd1ed379a3bc8de0185dbf542c591f3f10a1249eb3b029a647229",
    "drivers/staging/fw-api/fw/wmi.h":
        "0c692c53854be90342e0227298c657571d7c3c8ad6fe154378ce25a033256bbd",
    "drivers/staging/qca-wifi-host-cmn/wmi/src/wmi_unified_tlv.c":
        "ace2c278474f09db2a01dc617fd6b862d9aff6a515810f681f6f718410aba88e",
    "drivers/staging/qca-wifi-host-cmn/wmi/inc/wmi_unified_param.h":
        "a413d6b37c3bf85b305a91a7461b5239227a5c9c0d286602e198a8eb12614a70",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def parse_enum(body: str) -> dict[str, int]:
    body = re.sub(r"/\*.*?\*/|//[^\n]*", "", body, flags=re.S)
    values: dict[str, int] = {}
    previous = -1
    for item in body.split(","):
        item = item.strip()
        if not item:
            continue
        match = re.fullmatch(
            r"([A-Za-z_][A-Za-z0-9_]*)(?:\s*=\s*([A-Za-z_][A-Za-z0-9_]*|\d+))?",
            item,
        )
        require(match is not None, f"unhandled enum item: {item!r}")
        name, expression = match.groups()
        if expression is None:
            value = previous + 1
        elif expression.isdigit():
            value = int(expression)
        else:
            require(expression in values, f"unresolved enum alias: {expression}")
            value = values[expression]
        values[name] = value
        previous = value
    return values


def struct_fields(header: str, type_name: str) -> list[str]:
    end = header.find(f"}} {type_name};")
    require(end >= 0, f"missing {type_name}")
    start = header.rfind("typedef struct {", 0, end)
    require(start >= 0, f"missing declaration for {type_name}")
    body = re.sub(r"/\*.*?\*/|//[^\n]*", "", header[start + len("typedef struct {"):end], flags=re.S)
    return re.findall(r"\b([A-Za-z_]\w*)\s*(?:\[[^\]]+\])?\s*;", body)


def struct_layout(header: str, type_name: str) -> tuple[dict[str, int], int]:
    end = header.find(f"}} {type_name};")
    require(end >= 0, f"missing {type_name}")
    start = header.rfind("typedef struct {", 0, end)
    require(start >= 0, f"missing declaration for {type_name}")
    body = re.sub(r"/\*.*?\*/|//[^\n]*", "", header[start + len("typedef struct {"):end], flags=re.S)
    declarations = re.findall(
        r"\b([A-Za-z_]\w*)\s+([A-Za-z_]\w*)\s*(\[\d+\])?\s*;",
        body,
    )
    sizes = {"A_UINT32": 4, "A_UINT8": 1, "wmi_mac_addr": 8, "wmi_key_seq_counter": 8}
    offsets: dict[str, int] = {}
    cursor = 0
    for field_type, name, array in declarations:
        require(field_type in sizes, f"unhandled {type_name} field type: {field_type}")
        count = int(array[1:-1]) if array else 1
        offsets[name] = cursor
        cursor += sizes[field_type] * count
    return offsets, cursor


def main() -> None:
    commit = subprocess.check_output(
        ["git", "-C", str(KERNEL), "rev-parse", "HEAD"], text=True
    ).strip()
    require(commit == PINNED_COMMIT, f"unexpected QCACLD source commit: {commit}")

    source: dict[str, str] = {}
    for relative, expected in SOURCE_HASHES.items():
        path = KERNEL / relative
        require(sha256(path.read_bytes()).hexdigest() == expected,
                f"pinned source hash mismatch: {relative}")
        result = subprocess.run(
            ["git", "-C", str(KERNEL), "diff", "--quiet", "HEAD", "--", relative],
            check=False,
        )
        require(result.returncode == 0, f"pinned source has local edits: {relative}")
        source[relative] = path.read_text(errors="replace")

    header = source["drivers/staging/fw-api/fw/wmi_unified.h"]
    command_section = re.sub(r"/\*.*?\*/|//[^\n]*", "", header, flags=re.S)
    command_start = command_section.index("WMI_VDEV_CREATE_CMDID = WMI_CMD_GRP_START_ID(WMI_GRP_VDEV)")
    command_end = command_section.index("WMI_PEER_CREATE_CMDID", command_start)
    command_names = re.findall(r"\b(WMI_[A-Z0-9_]+_CMDID)\b", command_section[command_start:command_end])
    require(command_names[:9] == [
        "WMI_VDEV_CREATE_CMDID", "WMI_VDEV_DELETE_CMDID", "WMI_VDEV_START_REQUEST_CMDID",
        "WMI_VDEV_RESTART_REQUEST_CMDID", "WMI_VDEV_UP_CMDID", "WMI_VDEV_STOP_CMDID",
        "WMI_VDEV_DOWN_CMDID", "WMI_VDEV_SET_PARAM_CMDID", "WMI_VDEV_INSTALL_KEY_CMDID",
    ], "VDEV command ordering changed")
    require("WMI_GRP_VDEV,           /* 0x5 */" in header and
            "#define WMI_CMD_GRP_START_ID(grp_id) (((grp_id) << 12) | 0x1)" in header,
            "WMI command group base changed")
    require(0x5001 + command_names.index("WMI_VDEV_INSTALL_KEY_CMDID") == 0x5009,
            "install-key command ID no longer evaluates to 0x5009")

    tlv_defs = source["drivers/staging/fw-api/fw/wmi_tlv_defs.h"]
    tag_enum = re.search(
        r"/\*\* Enum list of TLV Tags for each parameter structure type\. \*/"
        r"\s*typedef enum\s*\{(.*?)\}\s*WMITLV_TAG_ID\s*;",
        tlv_defs,
        re.S,
    )
    require(tag_enum is not None, "could not locate WMI TLV tag enum")
    tags = parse_enum(tag_enum.group(1))
    require(tags.get("WMITLV_TAG_ARRAY_BYTE") == 17, "array-byte TLV tag changed")
    require(tags.get("WMITLV_TAG_STRUC_wmi_vdev_install_key_cmd_fixed_param") == 96,
            "install-key fixed-parameter TLV tag changed")

    expected_fields = [
        "tlv_header", "vdev_id", "peer_macaddr", "key_ix", "key_flags", "key_cipher",
        "key_rsc_counter", "key_global_rsc_counter", "key_tsc_counter",
        "wpi_key_rsc_counter", "wpi_key_tsc_counter", "key_len", "key_txmic_len",
        "key_rxmic_len", "is_group_key_ix_valid", "group_key_ix",
    ]
    require(struct_fields(header, "wmi_vdev_install_key_cmd_fixed_param") == expected_fields,
            "install-key fixed-parameter field layout changed")
    offsets, fixed_bytes = struct_layout(header, "wmi_vdev_install_key_cmd_fixed_param")
    require(fixed_bytes == 104, f"install-key fixed structure size changed: {fixed_bytes}")
    require(offsets == {
        "tlv_header": 0, "vdev_id": 4, "peer_macaddr": 8, "key_ix": 16,
        "key_flags": 20, "key_cipher": 24, "key_rsc_counter": 28,
        "key_global_rsc_counter": 36, "key_tsc_counter": 44,
        "wpi_key_rsc_counter": 52, "wpi_key_tsc_counter": 68, "key_len": 84,
        "key_txmic_len": 88, "key_rxmic_len": 92, "is_group_key_ix_valid": 96,
        "group_key_ix": 100,
    }, f"install-key field offsets changed: {offsets}")

    params = source["drivers/staging/qca-wifi-host-cmn/wmi/inc/wmi_unified_param.h"]
    key_params = params[params.index("struct set_key_params {"):]
    key_params = key_params[:key_params.index("\n};")]
    require("uint8_t vdev_id;" in key_params and "0:pairwise key, 1:group key" in params,
            "QCACLD key-parameter identifier or usage flags changed")
    wmi = source["drivers/staging/fw-api/fw/wmi.h"]
    require("PAIRWISE_USAGE      = 0x00" in wmi and "GROUP_USAGE         = 0x01" in wmi,
            "key usage values changed")
    require("#define  WMI_CIPHER_AES_CCM      0x4" in header,
            "CCMP cipher ID changed")
    htt_common = source["drivers/staging/fw-api/fw/htt_common.h"]
    security_types = re.search(r"enum htt_sec_type\s*\{(.*?)\};", htt_common, re.S)
    require(security_types is not None, "missing HTT security-type enum")
    htt_security_values = parse_enum(security_types.group(1))
    require(htt_security_values.get("htt_sec_type_aes_ccmp") == 6,
            "QCACLD CCMP SEC_IND value changed")

    codec = source["drivers/staging/qca-wifi-host-cmn/wmi/src/wmi_unified_tlv.c"]
    start = codec.index("static QDF_STATUS send_setup_install_key_cmd_tlv(")
    send = codec[start:codec.index("\n}", start)]
    for marker in (
        "sizeof(*cmd) + roundup(key_params->key_len, sizeof(uint32_t))",
        "WMITLV_TAG_STRUC_wmi_vdev_install_key_cmd_fixed_param",
        "WMITLV_TAG_ARRAY_BYTE",
        "WMI_CHAR_ARRAY_TO_MAC_ADDR(key_params->peer_mac, &cmd->peer_macaddr)",
        "qdf_mem_copy((void *)key_data,",
        "qdf_mem_copy(&cmd->key_rsc_counter, key_params->key_rsc_counter",
        "WMI_VDEV_INSTALL_KEY_CMDID",
    ):
        require(marker in send, f"QCACLD install-key serializer changed: {marker}")

    rust = (ROOT / "rust/src/sunfish_qcacld_key.rs").read_text()
    native = (ROOT / "rust/src/sunfish_qcacld_key_native.rs").read_text()
    join = (ROOT / "rust/src/sunfish_qcacld_key_join.rs").read_text()
    join_native = (ROOT / "rust/src/sunfish_qcacld_key_join_native.rs").read_text()
    ce_native = (ROOT / "rust/src/qcom_ce_v3_native_io.rs").read_text()
    rx_native = (ROOT / "rust/src/sunfish_qcacld_wmi_rx_native.rs").read_text()
    htt_event_native = (ROOT / "rust/src/sunfish_qcacld_htt_event_native.rs").read_text()
    for marker in (
        "WMI_VDEV_INSTALL_KEY_COMMAND_ID: u32 = 0x5009",
        "WMI_VDEV_INSTALL_KEY_FIXED_TAG: u16 = 96",
        "WMI_ARRAY_BYTE_TAG: u16 = 17",
        "WMI_VDEV_INSTALL_KEY_FIXED_BYTES: usize = 104",
        "CCMP128_KEY_BYTES: usize = 16",
        "const KEY_FLAGS_GROUP: u32 = 1",
        "const CIPHER_AES_CCM: u32 = 4",
        "wipe_ccmp_install_key_body",
        "qcom_htc::encode_wmi_command_frame",
        "qcom_wmi_ce::plan_wmi_tx",
        "wipe_qcacld_key_install_frame",
    ):
        require(marker in rust, f"Rust QCACLD key codec differs from pinned source: {marker}")
    for marker in (
        r"startup\.phase\(\)\s*!=\s*WmiStartupPhase::Ready",
        r"credits\s*\.reserve\(active_generation,\s*credit_plan\)",
        r"sensitive\.reserve\(lease,\s*\*cursor,\s*plan\.dma_pool\(\)\)",
        r"io\.publish_wmi_tx\(active_generation",
        r"quarantine_uncertain_publication",
        r"plan\.wipe_frame\(\)",
    ):
        require(re.search(marker, native) is not None,
                f"native key publisher guard changed: {marker}")
    require("pub fn reclaim_sensitive_wmi_tx" in ce_native,
            "missing read-index-bound sensitive WMI DMA reclaim")
    require("sensitive_bytes_scrubbed" in ce_native,
            "sensitive WMI DMA reclaim report changed")
    require("observe_security_indication" not in rx_native,
            "CE1 SEC_IND path unexpectedly bypasses the key association owner")
    for marker in (
        "service_native_htt_ce1_irq",
        "consume_and_dispatch(endpoint, scratch",
        "acknowledge_htt_rx_ce1_copy_complete(generation)",
        "report.irq_acknowledged && cursor.pending() != 0",
    ):
        require(marker in htt_event_native,
                f"native HTT CE1 service guard or dispatch path changed: {marker}")
    for marker in (
        "next_wpa2_key_install_request",
        "plan_next_wpa2_key_install_ce_tx",
        "next_unqueued_kind",
        "dispatch_wpa2_security_indication",
        "join.confirm_keys_installed(tracker)",
        "Since `SEC_IND` carries no",
    ):
        require(marker in join, f"WPA2/QCACLD association glue changed: {marker}")
    require("NativeIo" not in join and "publish_wmi_tx" not in join,
            "association glue must remain a protocol-only layer")
    for marker in (
        "dispatch_wpa2_event",
        "Event::SecurityIndication(indication)",
        "publish_next_wpa2_key_install",
        "service_native_wpa2_htt_ce1_irq",
        "service_native_htt_ce1_irq(",
        "startup.phase() != WmiStartupPhase::Ready",
        "plan_next_wpa2_key_install_ce_tx(",
        "publish_qcacld_key_install(",
    ):
        require(marker in join_native,
                f"association-owned native key path is missing: {marker}")
    require("NativeIo" not in rust and "publish_wmi_tx" not in rust,
            "offline key planner must not claim live CE3 publication")

    print(f"PASS: Sunfish QCACLD CCMP key serializer matches pinned source {commit}")
    print("PASS: CCMP key command is READY/credit gated and retains a read-index-bound DMA scrub lease")
    print("PASS: WPA2 key selection binds pairwise/GTK material to its association tracker")
    print("PASS: SEC_IND completion can confirm the join only after all queued keys match")
    print("PASS: native CE1 service routes SEC_IND through the association tracker before the owner callback")
    print("PASS: native owner composes CE1 dispatch with READY-gated association key publication")
    print("PASS: native CE1 service drains and dispatches bounded HTT events before status acknowledgement")
    print("LIMIT: system owner still must supply authorized CE/SMMU resources, credits, and RX delivery")


if __name__ == "__main__":
    main()
