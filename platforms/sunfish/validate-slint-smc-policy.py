#!/usr/bin/env python3
"""Fail the Slint build if generic SMC authority escapes the reset PD."""

from __future__ import annotations

import argparse
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SYSTEM = ROOT / "platforms" / "sunfish" / "slint-display.system"
TOUCH_SOURCE = ROOT / "platforms" / "sunfish" / "touch-service.c"


def validate(report_path: Path) -> None:
    system = ET.parse(SYSTEM).getroot()
    enabled = [
        pd.get("name")
        for pd in system.findall("protection_domain")
        if pd.get("smc", "false").lower() == "true"
    ]
    if enabled != ["slint_reset"]:
        raise ValueError(f"unexpected SMC-enabled protection domains: {enabled}")

    if "microkit_arm_smc_call" in TOUCH_SOURCE.read_text():
        raise ValueError("touch service contains a direct SMC call")

    report = report_path.read_text()
    cnode_blocks = re.finditer(
        r"(?ms)^\t- CNode: '(cnode_[^']+)'.*?(?=^\t- |\Z)", report
    )
    granted = {
        match.group(1)
        for match in cnode_blocks
        if "-> Object: 'arm_smc'" in match.group(0)
    }
    if granted != {"cnode_slint_reset"}:
        raise ValueError(f"generated SMC caps are granted to unexpected CSpaces: {sorted(granted)}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("report", type=Path, help="Microkit-generated report.txt")
    args = parser.parse_args()
    try:
        validate(args.report)
    except (OSError, ET.ParseError, ValueError) as error:
        print(f"ERROR Slint SMC capability policy: {error}", file=sys.stderr)
        return 1
    print("OK only the fixed-purpose slint_reset CSpace receives an SMC capability")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
