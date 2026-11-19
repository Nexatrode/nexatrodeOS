#!/usr/bin/env python3
"""Validate the event ordering in a filtered Sunfish Lineage boot capture."""

from __future__ import annotations

import argparse
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CAPTURE = (
    ROOT
    / "platforms/sunfish/fixtures/lineage-modem-startup-order-20261025.log"
)
SECOND_CAPTURE = (
    ROOT
    / "platforms/sunfish/fixtures/lineage-modem-startup-order-20261025-probe.log"
)
EVENTS = (
    ("mdm_helper_missing", "Cannot find '/vendor/bin/mdm_helper'"),
    ("per_mgr_started", "started service 'vendor.per_mgr'"),
    ("qrtr_started", "started service 'vendor.qrtr-ns'"),
    ("platform_tftp_started", "started service 'tftp_server'"),
    ("modem_svc_started", "started service 'modem_svc'"),
    ("pd_mapper_started", "started service 'pd_mapper'"),
    ("vendor_tftp_started", "started service 'vendor.tftp_server'"),
    ("per_proxy_started", "started service 'per_proxy'"),
    ("modem_firmware_selected", "Changing subsys fw_name to modem"),
    ("modem_mdt_requested", "firmware: loading 'modem.mdt'"),
    ("modem_mdt_found", "found /vendor/firmware_mnt/image/modem.mdt"),
    ("modem_reset_released", "modem: Brought out of reset"),
    ("modem_ready", "modem: Power/Clock ready interrupt received"),
    ("wlan_pd_up", "Indication received from msm/modem/wlan_pd, state: 0x1fffffff"),
    ("icnss_connected", "icnss_qmi: QMI Server Connected"),
    ("wlan_firmware_ready", "wma_wait_for_ready_event: 6853: FW ready event received"),
)
TIMESTAMP = re.compile(r"^\[\s*(\d+\.\d+)\]")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "captures",
        nargs="*",
        type=Path,
        default=[DEFAULT_CAPTURE, SECOND_CAPTURE],
    )
    args = parser.parse_args()

    for capture in args.captures:
        lines = capture.read_text(encoding="utf-8").splitlines()
        events: list[tuple[str, float]] = []
        for name, marker in EVENTS:
            matches = []
            for line in lines:
                if marker not in line:
                    continue
                timestamp = TIMESTAMP.match(line)
                if timestamp:
                    matches.append(float(timestamp.group(1)))
            if len(matches) != 1:
                raise SystemExit(
                    f"FAIL: expected one {name} record in {capture}, got {len(matches)}"
                )
            events.append((name, matches[0]))

        for (previous, previous_time), (current, current_time) in zip(
            events, events[1:]
        ):
            if previous_time >= current_time:
                raise SystemExit(
                    f"FAIL: {capture}: {current} ({current_time}) did not follow "
                    f"{previous} ({previous_time})"
                )
        print(f"PASS: {len(events)} selected Lineage startup events are ordered in {capture.name}")

    print(
        f"PASS: mdm_helper was missing in all {len(args.captures)} captures; "
        "modem and WLAN nevertheless reached reset-release, wlan_pd UP, and FW READY"
    )
    print(
        "LIMIT: timestamps do not identify which service initiated MPSS or "
        "launched wlan_pd, and do not authorize the seL4 caller"
    )


if __name__ == "__main__":
    main()
