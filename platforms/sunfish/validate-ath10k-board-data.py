#!/usr/bin/env python3
"""Audit the bounded board-2 parser against pinned Sunfish ath10k source."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "fd78d17954a86ea6ceb0c535e618ea129e09f8dd"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-linux-mainline-ref"
CORE = "drivers/net/wireless/ath/ath10k/core.c"
CORE_H = "drivers/net/wireless/ath/ath10k/core.h"
HW_H = "drivers/net/wireless/ath/ath10k/hw.h"
SUNFISH_DTS = "arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts"


def git(source: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(source), *args], text=True)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def section(text: str, start: str, end: str) -> str:
    begin = text.index(start)
    return text[begin : text.index(end, begin)]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    if not source.is_dir():
        raise SystemExit(f"FAIL: source tree not found: {source}")
    commit = git(source, "rev-parse", "HEAD").strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")

    paths = (CORE, CORE_H, HW_H, SUNFISH_DTS)
    for path in paths:
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", path],
            check=False,
        )
        require(result.returncode == 0, f"audited source file has local edits: {path}")
    core, core_h, hw_h, dts = (
        git(source, "show", f"HEAD:{path}") for path in paths
    )

    require('#define ATH10K_BOARD_MAGIC                  "QCA-ATH10K-BOARD"' in hw_h,
            "the pinned board-2 magic changed")
    require('#define ATH10K_BOARD_API2_FILE         "board-2.bin"' in hw_h,
            "the API-2 board filename changed")
    ie_struct = section(hw_h, "struct ath10k_fw_ie {", "};")
    require("__le32 id;" in ie_struct and "__le32 len;" in ie_struct,
            "the pinned board IE header is not little-endian {id,len}")
    for definition in (
        "ATH10K_BD_IE_BOARD = 0,",
        "ATH10K_BD_IE_BOARD_EXT = 1,",
        "ATH10K_BD_IE_BOARD_NAME = 0,",
        "ATH10K_BD_IE_BOARD_DATA = 1,",
    ):
        require(definition in hw_h, f"pinned board IE ID changed: {definition}")

    parse_board = section(
        core,
        "static int ath10k_core_parse_bd_ie_board(",
        "static int ath10k_core_search_bd(",
    )
    for marker in (
        "while (buf_len > sizeof(struct ath10k_fw_ie))",
        "buf_len < ALIGN(board_ie_len, 4)",
        "board_ie_len != strlen(boardname)",
        "memcmp(board_ie_data, boardname, strlen(boardname))",
        "name_match_found = true;",
        "case ATH10K_BD_IE_BOARD_DATA:",
        "if (!name_match_found)",
        "ar->normal_mode_fw.board_data = board_ie_data;",
        "goto out;",
    ):
        require(marker in parse_board, f"pinned board-entry parser behavior changed: {marker}")

    fetch_api2 = section(
        core,
        "static int ath10k_core_fetch_board_data_api_n(",
        "static int ath10k_core_create_board_name(",
    )
    require("strlen(ATH10K_BOARD_MAGIC) + 1" in fetch_api2
            and "ALIGN(magic_len, 4)" in fetch_api2
            and "ath10k_core_search_bd(ar, boardname, data, len)" in fetch_api2,
            "the API-2 magic padding/search sequence changed")
    fetch_file = section(core, "int ath10k_core_fetch_board_file(", "EXPORT_SYMBOL(ath10k_core_fetch_board_file);")
    ordered_names = (
        "sizeof(boardname), true,\n\t\t\t\t\t\t    true",
        "sizeof(boardname), false,\n\t\t\t\t\t\t    true",
        "sizeof(boardname), false,\n\t\t\t\t\t\t    false",
    )
    offsets = [fetch_file.find(marker) for marker in ordered_names]
    require(all(offset >= 0 for offset in offsets) and offsets == sorted(offsets),
            "the primary/first/second board-name fallback order changed")

    names = section(core, "static int ath10k_core_create_board_name(", "static int ath10k_core_create_eboard_name(")
    for marker in (
        '"bus=%s,qmi-board-id=%x,qmi-chip-id=%x%s"',
        '"bus=%s,qmi-board-id=%x"',
        '",variant=%s"',
    ):
        require(marker in names, f"pinned QMI board-name formatting changed: {marker}")
    require("#define ATH10K_SMBIOS_BDF_EXT_STR_LENGTH 0x20" in core_h,
            "the pinned BDF variant buffer bound changed")
    require("char boardname[100], fallback_boardname1[100], fallback_boardname2[100];"
            in fetch_file,
            "the pinned board-name formatting buffer bound changed")
    require('return "snoc";' in core_h, "the SNOC bus name changed")
    require('qcom,ath10k-calibration-variant = "google_sunfish";' in dts,
            "Sunfish's ath10k calibration variant changed")
    eboard_names = section(
        core,
        "static int ath10k_core_create_eboard_name(",
        "int ath10k_core_fetch_board_file(",
    )
    require('"bus=%s,bmi-chip-id=%d,bmi-eboard-id=%d"' in eboard_names
            and "ar->id.bmi_eboard_id" in eboard_names
            and "return -1;" in eboard_names,
            "extended-board selection no longer uses the separate BMI eboard identity")

    print(f"PASS: pinned Sunfish ath10k source {commit} and clean audited files")
    print("PASS: board-2 magic, endian IE header, four-byte padding, exact name/data ordering")
    print("PASS: SNOC QMI primary and two fallback names include google_sunfish in the source-defined slot")
    print("PASS: extended-board data remains on the separate BMI eboard-name path")
    print("LIMIT: selection does not prove that any licensed board file contains the selected Sunfish entry")


if __name__ == "__main__":
    main()
