#!/usr/bin/env python3
"""Audit the WMI 10.4 STOP_SCAN encoder against pinned Sunfish ath10k source."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "fd78d17954a86ea6ceb0c535e618ea129e09f8dd"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-linux-mainline-ref"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def section(source: str, start: str, end: str) -> str:
    begin = source.index(start)
    finish = source.index(end, begin)
    return source[begin:finish]


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
    relative_paths = (
        "drivers/net/wireless/ath/ath10k/wmi.c",
        "drivers/net/wireless/ath/ath10k/wmi.h",
    )
    for relative in relative_paths:
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", relative],
            check=False,
        )
        require(result.returncode == 0, f"pinned source has local edits: {relative}")

    ath = source / "drivers/net/wireless/ath/ath10k"
    wmi_c = (ath / "wmi.c").read_text()
    wmi_h = (ath / "wmi.h").read_text()
    rust = (ROOT / "rust/src/ath10k_scan.rs").read_text()

    command_ids = section(wmi_h, "enum wmi_10_4_cmd_id {", "};")
    require("WMI_10_4_START_CMDID = 0x9000" in command_ids,
            "WMI 10.4 start command base changed")
    require("WMI_10_4_START_SCAN_CMDID = WMI_10_4_START_CMDID" in command_ids,
            "WMI 10.4 start-scan command no longer uses the base ID")
    require("WMI_10_4_STOP_SCAN_CMDID," in command_ids,
            "WMI 10.4 stop-scan command ID is not immediately after start-scan")
    stop_map = section(wmi_c, "static struct wmi_cmd_map wmi_10_4_cmd_map = {", "};")
    require(".stop_scan_cmdid = WMI_10_4_STOP_SCAN_CMDID" in stop_map,
            "10.4 command map no longer selects WMI_10_4_STOP_SCAN_CMDID")

    stop_type = section(wmi_h, "enum wmi_stop_scan_type {", "};")
    require("WMI_SCAN_STOP_ONE\t= 0x00000000" in stop_type,
            "single-scan stop request type changed")
    stop_struct = section(wmi_h, "struct wmi_stop_scan_cmd {", "};")
    ordered_fields = (
        "__le32 scan_req_id;",
        "__le32 scan_id;",
        "__le32 req_type;",
        "__le32 vdev_id;",
    )
    cursor = 0
    for field in ordered_fields:
        index = stop_struct.find(field, cursor)
        require(index >= 0, f"stop command fields changed order or are missing: {field}")
        cursor = index + len(field)

    generator = section(wmi_c, "ath10k_wmi_op_gen_stop_scan(", "ath10k_wmi_op_gen_vdev_create(")
    for marker in (
        "scan_id |= WMI_HOST_SCAN_REQ_ID_PREFIX;",
        "req_id |= WMI_HOST_SCAN_REQUESTOR_ID_PREFIX;",
        "cmd->req_type    = __cpu_to_le32(arg->req_type);",
        "cmd->vdev_id     = __cpu_to_le32(arg->u.vdev_id);",
        "cmd->scan_id     = __cpu_to_le32(scan_id);",
        "cmd->scan_req_id = __cpu_to_le32(req_id);",
    ):
        require(marker in generator, f"pinned stop-scan generator changed: {marker}")

    require("pub const WMI_10_4_STOP_SCAN_COMMAND_ID: u32 = 0x9001;" in rust,
            "Rust stop-scan command ID no longer matches the pinned enum")
    encoder = section(rust, "pub fn encode_stop_scan(", "/// Queue the exact matching `STOP_SCAN`")
    for marker in (
        "session.requestor_id,",
        "session.scan_id,",
        "WMI_SCAN_STOP_ONE,",
        "session.vdev_id,",
    ):
        require(marker in encoder, f"Rust stop-scan body order changed: {marker}")

    print(f"PASS: pinned Sunfish ath10k source {commit}")
    print("PASS: WMI 10.4 STOP_SCAN ID, one-scan type, field order, and host prefixes")
    print("PASS: Rust encoder emits the matching requestor, scan, type, and vdev words")
    print("LIMIT: encoder/queue tests are host-only; no CE DMA or radio scan is performed")


if __name__ == "__main__":
    main()
