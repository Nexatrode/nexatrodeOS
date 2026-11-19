#!/usr/bin/env python3
"""Check Sunfish HTC receive flags and trailer-credit handling against pinned QCACLD."""

from __future__ import annotations

import argparse
import hashlib
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-lineage-kernel"
PATHS = {
    "htc_header": "drivers/staging/fw-api/fw/htc.h",
    "htc_recv": "drivers/staging/qca-wifi-host-cmn/htc/htc_recv.c",
    "htc_send": "drivers/staging/qca-wifi-host-cmn/htc/htc_send.c",
    "htc_services": "drivers/staging/qca-wifi-host-cmn/htc/htc_services.c",
    "htc": "drivers/staging/qca-wifi-host-cmn/htc/htc.c",
}
PINNED_SHA256 = {
    "htc_header": "b3face5874b7bfbe61637caa76d5381a32e3043654e2c476b5d0c10873b77800",
    "htc_recv": "e5d47842e40bb10e7156884e432098a5e771fcc3dedd644edbe430cce3128562",
    "htc_send": "b50e671becfd9922c939294aa686e91d6ee1d16e73e0b7fc49e87880ef2fb5e4",
    "htc_services": "f99ad98e7acaccc6b52a76b430921f24776fe326cec1033cb2eea36ac2c80dd7",
    "htc": "34643623d23451e6cf6f4e0f8133487c4e2486a08a2b23fc0accfc375b5802d0",
}


def git(source: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(source), *args], text=True)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    require(source.is_dir(), f"source tree not found: {source}")
    commit = git(source, "rev-parse", "HEAD").strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")

    sources: dict[str, str] = {}
    for name, path in PATHS.items():
        clean = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", path],
            check=False,
        )
        require(clean.returncode == 0, f"audited source file has local edits: {path}")
        text = git(source, "show", f"HEAD:{path}")
        digest = hashlib.sha256(text.encode()).hexdigest()
        require(digest == PINNED_SHA256[name], f"unexpected {name} source hash: {digest}")
        sources[name] = text

    header = sources["htc_header"]
    for macro, value in (
        ("HTC_FLAGS_NEED_CREDIT_UPDATE", "1 << 0"),
        ("HTC_FLAGS_RECV_1MORE_BLOCK", "1 << 0"),
        ("HTC_FLAGS_RECV_TRAILER", "1 << 1"),
    ):
        require(re.search(rf"#define\s+{macro}\s+\({re.escape(value)}\)", header) is not None,
                f"HTC direction-specific flag {macro} changed")
    require("PayloadLen : 16; /* length of data (including trailer)" in header,
            "HTC payload length no longer includes trailer bytes")
    require("ControlBytes0 : 8" in header and "ControlBytes1 : 8" in header,
            "HTC trailer-length/sequence header fields changed")
    require("#define HTC_RECORD_CREDITS          1" in header
            and "Credits : 8" in header,
            "HTC credit record ABI changed")

    receive = sources["htc_recv"]
    require("if (temp & HTC_FLAGS_RECV_TRAILER)" in receive
            and "HTC_FRAME_HDR,\n\t\t\t\t\t\t      CONTROLBYTES0" in receive,
            "HTC receive path no longer gates trailer length on receive trailer flag")
    trailer = receive[receive.index("static A_STATUS htc_process_trailer("):]
    require("case HTC_RECORD_CREDITS:" in trailer
            and "htc_process_credit_rpt(target" in trailer
            and "htc_rec_len /" in trailer
            and "sizeof(HTC_CREDIT_REPORT)" in trailer,
            "HTC receive trailer no longer decodes bounded credit records")

    send = sources["htc_send"]
    require("pEndpoint->TxCredits -= creditsRequired;" in send
            and "sendFlags |= HTC_FLAGS_NEED_CREDIT_UPDATE;" in send,
            "HTC TX credit reservation/request threshold changed")
    require("SM(pEndpoint->SeqNo, HTC_FRAME_HDR_CONTROLBYTES1)" in send
            and "pEndpoint->SeqNo++;" in send,
            "HTC per-endpoint sequence field behavior changed")

    services = sources["htc_services"]
    require("pEndpoint->TxCredits = txAlloc;" in services
            and "pEndpoint->TxCreditSize = target->TargetCreditSize;" in services,
            "HTC service connection credit initialization changed")
    target = sources["htc"]
    require("target->wmi_ep_count = 1;" in target
            and "credits = credits / target->wmi_ep_count;" in target
            and "pEntry->service_id = svc_id[i];" in target,
            "Sunfish WMI endpoint credit allocation assumptions changed")

    rust_htc = (ROOT / "rust/src/qcom_htc.rs").read_text()
    require("pub const HTC_FLAG_RECV_1MORE_BLOCK: u8 = 1 << 0;" in rust_htc
            and "pub const HTC_FLAG_RECV_TRAILER: u8 = 1 << 1;" in rust_htc,
            "Rust HTC receive flag constants drifted from pinned source")
    require("if flags & !HTC_FLAG_RECV_TRAILER != 0" in rust_htc
            and "parse_wmi_tx_frame" in rust_htc
            and "flags & !HTC_FLAG_NEED_CREDIT_UPDATE" in rust_htc,
            "Rust HTC send/receive flag directions are conflated")
    require("pub fn parse_rx_data_frame(" in rust_htc
            and "HtcCreditReportTotals" in rust_htc,
            "Rust HTC receive parser no longer preserves trailer credit reports")
    data_rx = (ROOT / "rust/src/sunfish_qcacld_htc.rs").read_text()
    require("parse_rx_data_frame(" in data_rx
            and "credit_update_requested" not in data_rx,
            "QCACLD data receive path reverted to send-direction credit flags")
    pipeline = (ROOT / "rust/src/sunfish_qcacld_rx_pipeline.rs").read_text()
    require("pub credit_returns: HtcCreditReportTotals" in pipeline,
            "HTC credit reports are no longer carried through the RX pipeline")

    print(f"Sunfish HTC credit ABI matches pinned QCACLD source {commit}")


if __name__ == "__main__":
    main()
