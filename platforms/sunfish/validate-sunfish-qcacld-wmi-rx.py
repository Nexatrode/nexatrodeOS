#!/usr/bin/env python3
"""Validate the WMI event IDs and fixed scan ABI against pinned Sunfish QCACLD."""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-lineage-kernel"
ABI_PATHS = (
    "drivers/staging/fw-api/fw/wmi_unified.h",
    "drivers/staging/fw-api/fw/wmi_tlv_defs.h",
)
WMA_PATH = "drivers/staging/qcacld-3.0/core/wma/src/wma_main.c"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def git(source: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(source), *args], text=True)


def clean_source(source: Path, relative: str) -> str:
    changed = subprocess.run(
        ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", relative],
        check=False,
    )
    require(changed.returncode == 0, f"pinned source has local edits: {relative}")
    return git(source, "show", f"HEAD:{relative}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    require(source.is_dir(), f"source tree not found: {source}")
    commit = git(source, "rev-parse", "HEAD").strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")
    unified, tlv_defs = (clean_source(source, path) for path in ABI_PATHS)
    wma_main = clean_source(source, WMA_PATH)
    uncommented_unified = re.sub(r"/\*.*?\*/", "", unified, flags=re.S)
    uncommented_unified = re.sub(r"//[^\n]*", "", uncommented_unified)

    require(re.search(r"WMI_SERVICE_READY_EVENTID\s*=\s*0x1\s*,", unified) is not None,
            "WMI SERVICE_READY event ID changed")
    service_ready_at = unified.index("WMI_SERVICE_READY_EVENTID = 0x1")
    event_start = unified.rfind("typedef enum {", 0, service_ready_at)
    event_ids = unified[event_start:]
    event_ids = event_ids[:event_ids.index("} WMI_EVT_ID")]
    uncommented_events = re.sub(r"/\*.*?\*/", "", event_ids, flags=re.S)
    uncommented_events = re.sub(r"//[^\n]*", "", uncommented_events)
    require(re.search(r"WMI_SERVICE_READY_EVENTID\s*=\s*0x1\s*,\s*"
                      r"WMI_READY_EVENTID\s*,\s*WMI_SERVICE_AVAILABLE_EVENTID", uncommented_events),
            "base WMI event ID ordering changed")
    require(re.search(r"WMI_GRP_START\s*=\s*0x3\s*,\s*"
                      r"WMI_GRP_SCAN\s*=\s*WMI_GRP_START", uncommented_unified),
            "WMI scan event group changed")
    require(re.search(r"WMI_GRP_PEER\s*,\s*WMI_GRP_MGMT", uncommented_unified),
            "WMI management event group ordering changed")
    require("#define WMI_EVT_GRP_START_ID(grp_id) (((grp_id) << 12) | 0x1)" in uncommented_unified,
            "WMI grouped event ID formula changed")
    require(re.search(r"WMI_SCAN_EVENTID\s*=\s*WMI_EVT_GRP_START_ID\(WMI_GRP_SCAN\)", uncommented_unified),
            "WMI scan event ID is no longer the scan-group base")
    mgmt_block = re.search(
        r"WMI_MGMT_RX_EVENTID\s*=\s*WMI_EVT_GRP_START_ID\(WMI_GRP_MGMT\),(.*?)"
        r"WMI_MGMT_TX_COMPLETION_EVENTID\s*,",
        uncommented_unified,
        re.S,
    )
    require(mgmt_block is not None, "management event ID order changed")
    mgmt_predecessors = re.findall(r"\bWMI_[A-Z0-9_]+\s*,", mgmt_block.group(1))
    require(len(mgmt_predecessors) == 4,
            f"unexpected management completion event offset: {len(mgmt_predecessors)}")
    require(0x7001 + len(mgmt_predecessors) + 1 == 0x7006,
            "management completion event ID no longer evaluates to 0x7006")

    tag_match = re.search(
        r"/\*\* Enum list of TLV Tags for each parameter structure type\. \*/"
        r"\s*typedef enum\s*\{(.*?)\}\s*WMITLV_TAG_ID\s*;",
        tlv_defs,
        re.S,
    )
    require(tag_match is not None, "could not locate pinned WMI TLV tag enum")
    tag_body = re.sub(r"/\*.*?\*/|//[^\n]*", "", tag_match.group(1), flags=re.S)
    tag_values: dict[str, int] = {}
    previous = -1
    for item in tag_body.split(","):
        item = item.strip()
        if not item:
            continue
        enum_item = re.fullmatch(
            r"([A-Za-z_][A-Za-z0-9_]*)(?:\s*=\s*([A-Za-z_][A-Za-z0-9_]*|\d+))?",
            item,
        )
        require(enum_item is not None, f"unhandled WMI TLV enum item: {item!r}")
        name, expression = enum_item.groups()
        if expression is None:
            value = previous + 1
        elif expression.isdigit():
            value = int(expression)
        else:
            require(expression in tag_values, f"unresolved WMI TLV alias: {expression}")
            value = tag_values[expression]
        tag_values[name] = value
        previous = value
    require(tag_values.get("WMITLV_TAG_STRUC_wmi_scan_event_fixed_param") == 36,
            "scan fixed-param TLV tag changed")
    require(tag_values.get("WMITLV_TAG_STRUC_wmi_mgmt_tx_compl_event_fixed_param") == 423,
            "management completion TLV tag changed")

    scan_struct_end = unified.find("} wmi_scan_event_fixed_param;")
    require(scan_struct_end >= 0, "scan fixed-parameter struct not found")
    scan_struct_start = unified.rfind("typedef struct {", 0, scan_struct_end)
    require(scan_struct_start >= 0, "scan fixed-parameter struct start not found")
    scan_struct = unified[scan_struct_start:scan_struct_end]
    fields = re.findall(r"A_UINT32\s+(\w+)\s*;", scan_struct)
    require(fields == [
        "tlv_header", "event", "reason", "channel_freq", "requestor",
        "scan_id", "vdev_id", "tsf_timestamp",
    ], f"scan fixed-parameter field order changed: {fields}")

    rust = (ROOT / "rust/src/sunfish_qcacld_wmi_rx.rs").read_text()
    for name, value in (
        ("WMI_SERVICE_READY_EVENT_ID", "0x0001"),
        ("WMI_READY_EVENT_ID", "0x0002"),
        ("WMI_SERVICE_AVAILABLE_EVENT_ID", "0x0003"),
        ("WMI_SCAN_EVENT_ID", "0x3001"),
        ("WMI_MGMT_TX_COMPLETION_EVENT_ID", "0x7006"),
        ("WMI_SCAN_EVENT_FIXED_PARAM_TLV_TAG", "36"),
        ("WMI_SCAN_EVENT_FIXED_PARAM_VALUE_BYTES", "28"),
    ):
        require(re.search(rf"pub const {name}: (?:u16|u32|usize) = {value};", rust) is not None,
                f"Rust {name} differs from the pinned ABI")
    require("qcom_htc::parse_wmi_frame(bytes, expected_endpoint_id, max_message_size)" in rust,
            "WMI dispatch no longer validates the HTC receive frame first")
    require("apply_credit_returns(generation, credit_returns)" in rust,
            "WMI dispatch no longer applies the validated HTC credit trailer")
    require("sunfish_qcacld_mgmt_tx::parse_wmi_mgmt_tx_completion(frame.tlv_body)" in rust,
            "management completion no longer uses the shared strict decoder")
    require("ServiceReadyView::parse(frame.tlv_body)" in rust
            and "ServiceAvailableView::parse(frame.tlv_body)" in rust,
            "service discovery no longer uses QCACLD's pinned event layouts")
    require("WmiReadyEvent::parse(frame.tlv_body)" in rust
            and "impl<'a> WmiReadyEvent<'a>" in (ROOT / "rust/src/qcacld_wmi.rs").read_text(),
            "READY no longer uses the strict QCACLD fixed structure and ABI policy")
    require("pub fn consume_dispatch_and_retire_mgmt_tx" in rust,
            "validated WMI completions are no longer connected to lease retirement")
    require("pub fn consume_dispatch_and_observe_startup" in rust
            and re.search(r"startup\s*\.observe\(&retired\.received\)", rust) is not None,
            "bounded CE2 event dispatch no longer advances the generation-bound startup session")
    require("pub struct ServiceReadySnapshot" in (ROOT / "rust/src/qcacld_wmi.rs").read_text()
            and "pub struct ServiceAvailableSnapshot" in (ROOT / "rust/src/qcacld_wmi.rs").read_text()
            and "service_ready_snapshot" in rust
            and "service_available_snapshot" in rust,
            "accepted startup service maps are no longer retained in owned validated snapshots")
    require(re.search(r"\.complete\(\s*generation,\s*completion\s*\)", rust) is not None,
            "management lease retirement is no longer generation-bound")
    require("ManagementTxRetirement(MgmtTxError)" in rust,
            "unknown or stale management completions are not surfaced as retirement errors")
    require(re.search(
        r"case wmi_service_ready_event_id:.*?"
        r"case wmi_service_ready_ext_event_id:.*?"
        r"case wmi_ready_event_id:",
        wma_main,
        re.S,
    ) is not None, "pinned WMA event routing order changed")
    service_ready_ext = wma_main[
        wma_main.index("int wma_rx_service_ready_ext_event("):
        wma_main.index("int wma_rx_ready_event(")
    ]
    require("WMA --> WMI_INIT_CMDID" in service_ready_ext,
            "pinned WMA no longer submits WMI_INIT from SERVICE_READY_EXT")
    ready_handler = wma_main[
        wma_main.index("int wma_rx_ready_event("):
        wma_main.index("void wma_setneedshutdown(")
    ]
    require("wma_handle->wmi_ready = true;" in ready_handler
            and "wma_handle->wlan_init_status = ev->status;" in ready_handler,
            "pinned WMA READY status handling changed")
    for marker in (
        "pub struct SunfishWmiStartupSession",
        "pub fn mark_init_submitted",
        "WmiStartupPhase::AwaitReady, WMI_READY_EVENT_ID",
    ):
        require(marker in rust, f"generation-bound WMI startup gate lost {marker}")
    require("sunfish_startup_snapshot()" in rust
            and re.search(
                r"pub fn sunfish_startup_snapshot\(.*?self\.require_sunfish_wmi_startup\(\)",
                (ROOT / "rust/src/qcacld_wmi.rs").read_text(),
                re.S,
            ) is not None,
            "startup gate no longer retains policy-validated Sunfish SERVICE_READY evidence")
    print(f"PASS: WMI event IDs and fixed event TLVs match pinned QCACLD source {commit}")


if __name__ == "__main__":
    main()
