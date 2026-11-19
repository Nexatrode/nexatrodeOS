#!/usr/bin/env python3
"""Check the Sunfish QCACLD management-TX codec against pinned source."""

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
    "drivers/staging/fw-api/fw/wmi_services.h":
        "ab8596477e75a8defc1295a867433db6111cd674fed0e7912a5327d8441d75e8",
    "drivers/staging/qca-wifi-host-cmn/wmi/src/wmi_unified_tlv.c":
        "ace2c278474f09db2a01dc617fd6b862d9aff6a515810f681f6f718410aba88e",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def main() -> None:
    commit = subprocess.check_output(
        ["git", "-C", str(KERNEL), "rev-parse", "HEAD"], text=True
    ).strip()
    require(commit == PINNED_COMMIT, f"unexpected QCACLD source commit: {commit}")

    source = {}
    for relative, expected in SOURCE_HASHES.items():
        path = KERNEL / relative
        actual = sha256(path.read_bytes()).hexdigest()
        require(actual == expected, f"pinned source hash mismatch: {relative}")
        source[relative] = path.read_text(errors="replace")

    header = source["drivers/staging/fw-api/fw/wmi_unified.h"]
    require("WMI_GRP_MGMT,           /* 0x7 */" in header,
            "WMI management command/event group changed")
    require("#define WMI_CMD_GRP_START_ID(grp_id) (((grp_id) << 12) | 0x1)" in header,
            "WMI command group base changed")
    command_block = re.search(
        r"WMI_BCN_TX_CMDID\s*=\s*WMI_CMD_GRP_START_ID\(WMI_GRP_MGMT\),(.*?)"
        r"WMI_MGMT_TX_SEND_CMDID\s*,",
        re.sub(r"/\*.*?\*/|//[^\n]*", "", header, flags=re.S),
        re.S,
    )
    require(command_block is not None,
            "management-TX command is no longer in the audited group order")
    command_predecessors = re.findall(r"\bWMI_[A-Z0-9_]+\s*,", command_block.group(1))
    require(len(command_predecessors) == 6,
            f"unexpected management-TX command offset: {len(command_predecessors)}")
    require(0x7001 + len(command_predecessors) + 1 == 0x7008,
            "management-TX command ID no longer evaluates to 0x7008")

    event_block = re.search(
        r"WMI_MGMT_RX_EVENTID\s*=\s*WMI_EVT_GRP_START_ID\(WMI_GRP_MGMT\),(.*?)"
        r"WMI_MGMT_TX_COMPLETION_EVENTID\s*,",
        re.sub(r"/\*.*?\*/|//[^\n]*", "", header, flags=re.S),
        re.S,
    )
    require(event_block is not None, "management-TX completion event order changed")
    event_predecessors = re.findall(r"\bWMI_[A-Z0-9_]+\s*,", event_block.group(1))
    require(len(event_predecessors) == 4,
            f"unexpected management-TX completion event offset: {len(event_predecessors)}")
    require(0x7001 + len(event_predecessors) + 1 == 0x7006,
            "management-TX completion event ID no longer evaluates to 0x7006")

    services = source["drivers/staging/fw-api/fw/wmi_services.h"]
    require("WMI_SERVICE_MGMT_TX_WMI=76" in services,
            "WMI management-TX service index changed")

    tlv_defs = source["drivers/staging/fw-api/fw/wmi_tlv_defs.h"]
    tag_enum = re.search(
        r"/\*\* Enum list of TLV Tags for each parameter structure type\. \*/"
        r"\s*typedef enum\s*\{(.*?)\}\s*WMITLV_TAG_ID\s*;",
        tlv_defs,
        re.S,
    )
    require(tag_enum is not None, "could not locate pinned WMI TLV tag enum")
    tag_body = re.sub(r"/\*.*?\*/|//[^\n]*", "", tag_enum.group(1), flags=re.S)
    tag_values = {}
    previous = -1
    for item in tag_body.split(","):
        item = item.strip()
        if not item:
            continue
        match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*)(?:\s*=\s*([A-Za-z_][A-Za-z0-9_]*|\d+))?", item)
        require(match is not None, f"unhandled TLV enum item: {item!r}")
        name, expression = match.groups()
        if expression is None:
            value = previous + 1
        elif expression.isdigit():
            value = int(expression)
        else:
            require(expression in tag_values, f"unresolved TLV enum alias: {expression}")
            value = tag_values[expression]
        tag_values[name] = value
        previous = value
    require(tag_values.get("WMITLV_TAG_STRUC_wmi_service_ready_event_fixed_param") == 32,
            "first WMI structure tag moved from the audited value 32")
    require(tag_values.get("WMITLV_TAG_STRUC_wmi_mgmt_tx_send_cmd_fixed_param") == 422,
            "management-TX command TLV tag no longer evaluates to 422")
    require(tag_values.get("WMITLV_TAG_STRUC_wmi_mgmt_tx_compl_event_fixed_param") == 423,
            "management-TX completion TLV tag no longer evaluates to 423")

    def struct_fields(type_name: str) -> list[str]:
        end = header.find(f"}} {type_name};")
        require(end >= 0, f"missing struct {type_name}")
        start = header.rfind("typedef struct {", 0, end)
        require(start >= 0, f"missing struct declaration for {type_name}")
        body = re.sub(
            r"/\*.*?\*/|//[^\n]*", "", header[start + len("typedef struct {"):end], flags=re.S
        )
        return re.findall(r"\bA_UINT32\s+(\w+)\s*;", body)

    send_fields = [
        "tlv_header", "vdev_id", "desc_id", "chanfreq", "paddr_lo", "paddr_hi",
        "frame_len", "buf_len", "tx_params_valid",
    ]
    completion_fields = [
        "tlv_header", "desc_id", "status", "pdev_id", "ppdu_id", "ack_rssi",
        "tx_rate", "peer_phymode", "retries_count", "tx_tsf_l32", "tx_tsf_u32",
    ]
    require(struct_fields("wmi_mgmt_tx_send_cmd_fixed_param") == send_fields,
            "management-TX send structure field order changed")
    require(struct_fields("wmi_mgmt_tx_compl_event_fixed_param") == completion_fields,
            "management-TX completion structure field order changed")

    completion_status = re.search(
        r"typedef enum \{(.*?)\}\s*WMI_MGMT_TX_COMP_STATUS_TYPE\s*;",
        header,
        re.S,
    )
    require(completion_status is not None, "missing management-TX completion status enum")
    status_names = re.findall(
        r"\b(WMI_MGMT_TX_COMP_TYPE_[A-Z_]+)\b", completion_status.group(1)
    )
    require(status_names == [
        "WMI_MGMT_TX_COMP_TYPE_COMPLETE_OK",
        "WMI_MGMT_TX_COMP_TYPE_DISCARD",
        "WMI_MGMT_TX_COMP_TYPE_INSPECT",
        "WMI_MGMT_TX_COMP_TYPE_COMPLETE_NO_ACK",
        "WMI_MGMT_TX_COMP_TYPE_MAX",
    ], "management-TX completion status values or order changed")

    codec = source["drivers/staging/qca-wifi-host-cmn/wmi/src/wmi_unified_tlv.c"]
    send = codec[codec.index("static QDF_STATUS send_mgmt_cmd_tlv("):]
    send = send[:send.index("\n}\n")]
    for fragment in (
        "qdf_nbuf_map_single(qdf_ctx, param->tx_frame",
        "qdf_nbuf_get_frag_paddr(param->tx_frame, 0)",
        "cmd->paddr_lo = (uint32_t)(dma_addr & 0xffffffff)",
        "cmd->paddr_hi = (uint32_t)((dma_addr >> 32) & 0x1F)",
        "cmd->frame_len = param->frm_len",
        "cmd->buf_len = bufp_len",
        "WMI_MGMT_TX_SEND_CMDID",
    ):
        require(fragment in send, f"QCACLD serializer changed: {fragment}")

    rust = (ROOT / "rust/src/sunfish_qcacld_mgmt_tx.rs").read_text()
    for fragment in (
        "WMI_MGMT_TX_WMI_SERVICE_INDEX: usize = 76",
        "WMI_MGMT_TX_SEND_COMMAND_ID: u32 = 0x7008",
        "WMI_MGMT_TX_COMPLETION_EVENT_ID: u32 = 0x7006",
        "WMI_MGMT_TX_FIXED_PARAM_TAG: u16 = 422",
        "WMI_MGMT_TX_COMPLETION_TLV_TAG: u16 = 423",
        "WMI_MGMT_TX_COMPLETION_VALUE_BYTES: usize = 40",
    ):
        require(fragment in rust, f"Rust codec source no longer matches: {fragment}")

    print("Sunfish QCACLD management-TX command, completion, capability, and lease ABI match the pinned source.")


if __name__ == "__main__":
    main()
