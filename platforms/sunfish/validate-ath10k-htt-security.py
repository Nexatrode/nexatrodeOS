#!/usr/bin/env python3
"""Audit the HTT security-indication parser against pinned Sunfish ath10k."""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "fd78d17954a86ea6ceb0c535e618ea129e09f8dd"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-linux-mainline-ref"
PINNED_QCACLD_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
DEFAULT_QCACLD_SOURCE = ROOT / ".cache/sunfish-lineage-kernel"


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
    parser.add_argument("--qcacld-source", type=Path, default=DEFAULT_QCACLD_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    if not source.is_dir():
        raise SystemExit(f"FAIL: source tree not found: {source}")
    commit = subprocess.check_output(
        ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
    ).strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")

    ath = source / "drivers/net/wireless/ath/ath10k"
    relative_paths = (
        "drivers/net/wireless/ath/ath10k/htt.h",
        "drivers/net/wireless/ath/ath10k/htt.c",
        "drivers/net/wireless/ath/ath10k/htt_rx.c",
    )
    for relative in relative_paths:
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", relative],
            check=False,
        )
        require(result.returncode == 0, f"pinned source has local edits: {relative}")

    htt_h = (ath / "htt.h").read_text()
    htt_c = (ath / "htt.c").read_text()
    htt_rx = (ath / "htt_rx.c").read_text()
    rust = (ROOT / "rust/src/ath10k_htt.rs").read_text()
    rx_ind_rust = (ROOT / "rust/src/sunfish_qcacld_rx_ind.rs").read_text()
    key_rust = (ROOT / "rust/src/ath10k_key.rs").read_text()

    enum = section(htt_h, "enum htt_10_4_t2h_msg_type {", "};")
    require("HTT_10_4_T2H_MSG_TYPE_SEC_IND                = 0xb" in enum,
            "WMI 10.4 security-indication message ID changed")
    main_enum = section(htt_h, "enum htt_t2h_msg_type {", "};")
    require("HTT_T2H_MSG_TYPE_SEC_IND," in main_enum,
            "common HTT security-indication message kind disappeared")
    require("[HTT_10_4_T2H_MSG_TYPE_SEC_IND] = HTT_T2H_MSG_TYPE_SEC_IND" in htt_c,
            "10.4 message-type translation no longer maps SEC_IND")

    require("HTT_SECURITY_TYPE_MASK 0x7F" in htt_h and
            "HTT_SECURITY_IS_UNICAST = 1 << 7" in htt_h,
            "security-indication flags changed")
    require("HTT_SECURITY_AES_CCMP," in section(htt_h, "enum htt_security_types {", "};"),
            "CCMP security type disappeared")
    indication = section(htt_h, "struct htt_security_indication {", "};")
    fields = (
        "u8 flags;",
        "__le16 peer_id;",
        "u8 michael_key[8];",
        "u8 wapi_rsc[16];",
    )
    cursor = 0
    for marker in fields:
        index = indication.find(marker, cursor)
        require(index >= 0, f"security-indication fields changed order or are missing: {marker}")
        cursor = index + len(marker)

    event_case = section(htt_rx, "case HTT_T2H_MSG_TYPE_SEC_IND:",
                         "case HTT_T2H_MSG_TYPE_RX_FRAG_IND:")
    require("ath10k_htt_rx_sec_ind_handler(ar, ev);" in event_case and
            "complete(&ar->install_key_done);" in event_case,
            "Linux no longer updates security state and releases the key-install waiter")

    for marker in (
        "HTT_10_4_T2H_SECURITY_INDICATION: u8 = 0x0b",
        "HTT_SECURITY_INDICATION_BYTES: usize = 28",
        "HTT_SECURITY_TYPE_AES_CCMP: u8 = 6",
        "flags & HTT_SECURITY_TYPE_MASK",
        "flags & HTT_SECURITY_IS_UNICAST != 0",
    ):
        require(marker in rust, f"Rust SEC_IND parser differs from source: {marker}")
    require(
        "HTT_T2H_MSG_TYPE_RX_IND: u8 = 0x01" in rx_ind_rust,
        "Rust RX_IND message ID differs from source",
    )
    for marker in (
        "indication.peer_id == expected_peer_id",
        "indication.security_type == HTT_SECURITY_TYPE_AES_CCMP",
        "indication.is_unicast == expected_unicast",
    ):
        require(marker in key_rust, f"Rust CCMP completion matcher lost a fail-closed check: {marker}")

    qcacld_source = args.qcacld_source.resolve()
    if not qcacld_source.is_dir():
        raise SystemExit(f"FAIL: QCACLD source tree not found: {qcacld_source}")
    qcacld_commit = subprocess.check_output(
        ["git", "-C", str(qcacld_source), "rev-parse", "HEAD"], text=True
    ).strip()
    require(
        qcacld_commit == PINNED_QCACLD_COMMIT,
        f"expected QCACLD source {PINNED_QCACLD_COMMIT}, got {qcacld_commit}",
    )
    qcacld_paths = (
        "drivers/staging/fw-api/fw/htt.h",
        "drivers/staging/fw-api/fw/htt_common.h",
        "drivers/staging/qcacld-3.0/core/dp/htt/htt_t2h.c",
        "drivers/staging/qcacld-3.0/core/wma/src/wma_mgmt.c",
    )
    for relative in qcacld_paths:
        result = subprocess.run(
            ["git", "-C", str(qcacld_source), "diff", "--quiet", "HEAD", "--", relative],
            check=False,
        )
        require(result.returncode == 0, f"pinned QCACLD source has local edits: {relative}")

    fw_htt = (qcacld_source / "drivers/staging/fw-api/fw/htt.h").read_text()
    fw_htt_common = (
        qcacld_source / "drivers/staging/fw-api/fw/htt_common.h"
    ).read_text()
    qcacld_t2h = (
        qcacld_source
        / "drivers/staging/qcacld-3.0/core/dp/htt/htt_t2h.c"
    ).read_text()
    wma = (
        qcacld_source
        / "drivers/staging/qcacld-3.0/core/wma/src/wma_mgmt.c"
    ).read_text()
    require("HTT_T2H_MSG_TYPE_RX_IND                   = 0x1" in fw_htt,
            "QCACLD RX_IND message ID changed")
    require("HTT_T2H_MSG_TYPE_SEC_IND                  = 0xb" in fw_htt,
            "QCACLD SEC_IND message ID changed")
    htt_sec_types = section(fw_htt_common, "enum htt_sec_type {", "};")
    htt_sec_type_names = re.findall(r"^\s*(htt_sec_type_[a-z0-9_]+)\s*,", htt_sec_types, re.M)
    require(len(htt_sec_type_names) > 6 and htt_sec_type_names[6] == "htt_sec_type_aes_ccmp",
            "QCACLD CCMP SEC_IND cipher value changed")
    for marker in (
        "#define HTT_SEC_IND_SEC_TYPE_M     0x00007f00",
        "#define HTT_SEC_IND_SEC_TYPE_S     8",
        "#define HTT_SEC_IND_UNICAST_M      0x00008000",
        "#define HTT_SEC_IND_UNICAST_S      15",
        "#define HTT_SEC_IND_PEER_ID_M      0xffff0000",
        "#define HTT_SEC_IND_PEER_ID_S      16",
        "#define HTT_SEC_IND_BYTES 28",
    ):
        require(marker in fw_htt, f"QCACLD SEC_IND wire definition changed: {marker}")
    sec_case = section(
        qcacld_t2h,
        "case HTT_T2H_MSG_TYPE_SEC_IND:",
        "case HTT_T2H_MSG_TYPE_MGMT_TX_COMPL_IND:",
    )
    for marker in (
        "HTT_SEC_IND_PEER_ID_GET(*msg_word)",
        "HTT_SEC_IND_SEC_TYPE_GET(*msg_word)",
        "HTT_SEC_IND_UNICAST_GET(*msg_word)",
        "msg_word++;",
        "ol_rx_sec_ind_handler(pdev->txrx_pdev",
    ):
        require(marker in sec_case, f"QCACLD SEC_IND dispatch changed: {marker}")
    require(
        "Should we wait till we get HTT_T2H_MSG_TYPE_SEC_IND?" in wma,
        "QCACLD WMA key-setup/SEC_IND asynchronous boundary changed",
    )

    event_rust = (ROOT / "rust/src/sunfish_qcacld_htt_event.rs").read_text()
    for marker in (
        "HTT_T2H_MSG_TYPE_RX_IND => Event::RxIndication(",
        "HTT_10_4_T2H_SECURITY_INDICATION => Event::SecurityIndication(",
        "HttSecurityIndication::parse(frame.htt_payload)",
        "frame.credit_returns",
    ):
        require(marker in event_rust, f"Rust CE1 HTT event dispatcher changed: {marker}")

    print(f"PASS: pinned Sunfish ath10k source {commit}")
    print("PASS: WMI 10.4 SEC_IND type, 28-byte message, peer/cipher/cast fields")
    print(f"PASS: pinned Lineage QCACLD source {qcacld_commit}")
    print("PASS: QCACLD CE1 RX_IND/SEC_IND IDs, packed fields, and 28-byte layout match Rust dispatcher")
    print("PASS: Linux completes the install-key waiter from SEC_IND")
    print("LIMIT: dispatcher tests do not run in the live CE1 owner or confirm keys")


if __name__ == "__main__":
    main()
