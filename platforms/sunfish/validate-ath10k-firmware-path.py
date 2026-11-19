#!/usr/bin/env python3
"""Audit the pinned Sunfish ath10k firmware-name lookup without booting WLAN."""

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
    require(commit == PINNED_COMMIT, f"expected pinned kernel {PINNED_COMMIT}, got {commit}")

    audited_paths = (
        "drivers/net/wireless/ath/ath10k/snoc.c",
        "drivers/net/wireless/ath/ath10k/core.c",
        "arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts",
    )
    for relative in audited_paths:
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", relative],
            check=False,
        )
        require(result.returncode == 0, f"audited source file has local edits: {relative}")

    snoc = (source / audited_paths[0]).read_text()
    core = (source / audited_paths[1]).read_text()
    dts = (source / audited_paths[2]).read_text()

    selector = 'of_property_read_string(dev->of_node, "firmware-name", &ar->board_name);'
    require(selector in snoc, "ath10k SNOC does not read firmware-name into board_name")

    begin = core.index("static const struct firmware *ath10k_fetch_fw_file(")
    end = core.index("\nstatic int ath10k_push_board_ext_data", begin)
    fetch = core[begin:end]
    board_path = 'snprintf(filename, sizeof(filename), "%s/%s/%s",\n\t\t\t dir, ar->board_name, file);'
    generic_path = 'snprintf(filename, sizeof(filename), "%s/%s", dir, file);'
    require("if (ar->board_name)" in fetch, "core firmware helper lacks optional subdirectory lookup")
    require(board_path in fetch, "core firmware helper's board_name request path changed")
    require(generic_path in fetch, "core firmware helper's generic fallback path changed")
    require(fetch.index(board_path) < fetch.index(generic_path), "generic path is tried before firmware-name path")
    require(fetch.index("if (!ret)\n\t\t\treturn fw;") < fetch.index(generic_path), "subdirectory result is not returned before fallback")

    api2_begin = core.index("static int ath10k_core_fetch_board_data_api_n(")
    api2_end = core.index("\nint ath10k_core_fetch_board_file", api2_begin)
    api2 = core[api2_begin:api2_end]
    require("ath10k_fetch_fw_file(ar," in api2,
            "board-2.bin is no longer loaded through the path-aware helper")
    api2_use = core.index("ret = ath10k_core_fetch_board_data_api_n(")
    api2_call = core[api2_use:core.index("if (!ret)", api2_use)]
    require("ATH10K_BOARD_API2_FILE" in api2_call,
            "API-2 board data no longer requests board-2.bin")
    firmware_begin = core.index("int ath10k_core_fetch_firmware_api_n(")
    firmware_end = core.index("\nEXPORT_SYMBOL", firmware_begin)
    firmware_load = core[firmware_begin:firmware_end]
    require("fw_file->firmware = ath10k_fetch_fw_file(ar, ar->hw_params.fw.dir," in firmware_load,
            "API-5 firmware image is no longer loaded through the path-aware helper")

    wifi_begin = dts.index("&wifi {")
    wifi_end = dts.index("\n};", wifi_begin)
    wifi = dts[wifi_begin:wifi_end]
    require('qcom,ath10k-calibration-variant = "google_sunfish";' in wifi,
            "Sunfish calibration variant changed")
    require("firmware-name" not in wifi,
            "current Sunfish DTS now has a firmware-name override; update the audit")

    print(f"PASS: pinned source {commit}")
    print("PASS: optional firmware-name subdirectory is tried before the generic path")
    print("PASS: API-5 firmware and board-2.bin use the path-aware helper")
    print("PASS: current Sunfish DTS leaves firmware-name unset and keeps google_sunfish calibration")
    print("LIMIT: source lookup is validated; image compatibility, board match, WLFW, MSA and DMA are not")


if __name__ == "__main__":
    main()
