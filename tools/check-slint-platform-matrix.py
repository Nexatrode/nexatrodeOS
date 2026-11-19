#!/usr/bin/env python3
"""Check that the documented Slint platform matrix matches build inputs."""

import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MATRIX = ROOT / "platforms/slint-build-matrix.json"


def require_file(relative_path: str) -> Path:
    path = ROOT / relative_path
    if not path.is_file():
        raise SystemExit(f"matrix references missing file: {relative_path}")
    return path


def page_size(target_path: Path) -> int:
    config = json.loads(target_path.read_text())
    match = re.search(r"~\((\d+)\s*-\s*1\)", config.get("link-script", ""))
    if match is None:
        raise SystemExit(f"cannot determine page size from {target_path.relative_to(ROOT)}")
    return int(match.group(1))


def main() -> None:
    matrix = json.loads(MATRIX.read_text())
    shared = matrix["shared"]
    for key in ("ui", "screen_wiring", "cargo_package"):
        require_file(shared[key])

    cargo_toml = require_file(shared["cargo_package"]).read_text()
    if f'{shared["common_feature"]} = []' not in cargo_toml:
        raise SystemExit("shared Slint feature is missing from Cargo.toml")
    profile_match = re.search(
        r'^full-suite-profile\s*=\s*\[([^\]]*)\]', cargo_toml, re.MULTILINE
    )
    if profile_match is None or f'"{shared["common_feature"]}"' not in profile_match.group(1):
        raise SystemExit("iPhone full-suite-profile no longer includes the shared UI")

    ids = set()
    for target in matrix["targets"]:
        target_id = target["id"]
        if target_id in ids:
            raise SystemExit(f"duplicate target id: {target_id}")
        ids.add(target_id)
        config_path = require_file(target["kernel_target"])
        script_path = require_file(target["entrypoint"])
        actual_page_size = page_size(config_path)
        if actual_page_size != target["page_size_bytes"]:
            raise SystemExit(
                f"{target_id}: matrix says {target['page_size_bytes']} byte pages, "
                f"target says {actual_page_size}"
            )

        if target_id == "utm-qemu-virt":
            dev_script = script_path.read_text()
            if '"test-slint-utm"' not in dev_script:
                raise SystemExit("UTM guest Slint test command is missing from dev")
            if target["interactive_guest_slint"] and not all(
                marker in dev_script for marker in (
                    '("home", (80, 118))', '("notes", (26, 277))',
                    '"calculator_3"', '"contacts_q"', '"calendar_q"',
                    '"messages_qw"', '"phone_call"',
                )
            ):
                raise SystemExit("UTM guest interaction check no longer covers its app-control smoke path")
        elif target_id == "pixel-4a-sunfish":
            if shared["common_feature"] not in script_path.read_text():
                raise SystemExit("Pixel build script does not select the shared UI")
        elif target_id == "iphone-7-plus":
            if "full-suite-profile" not in script_path.read_text():
                raise SystemExit("iPhone build script does not select the full suite")

        print(
            f"{target_id}: {actual_page_size // 1024} KiB pages, "
            f"{target['slint_mode']}, {target['build_check']}"
        )

    if ids != {"utm-qemu-virt", "pixel-4a-sunfish", "iphone-7-plus"}:
        raise SystemExit(f"unexpected target set: {sorted(ids)}")


if __name__ == "__main__":
    main()
