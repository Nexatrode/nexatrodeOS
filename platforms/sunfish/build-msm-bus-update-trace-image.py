#!/usr/bin/env python3
"""Build a temporary-boot Sunfish image with MSM-bus request debug enabled."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_REFERENCE = ROOT / ".cache/pixel-sunfish/lineage-current-fastboot.img"
DEFAULT_OUTPUT = ROOT / ".cache/pixel-sunfish/msm-bus-update-trace/fastboot.img"
DEBUG_ARGUMENT = (
    'dyndbg="func update_request_adhoc +p; func set_bw +p" log_buf_len=4M'
)


def packer_module():
    path = Path(__file__).with_name("repack-boot-v2.py")
    spec = importlib.util.spec_from_file_location("sunfish_boot_v2_packer", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load boot-image packer: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def command_line(header: bytes) -> bytes:
    return (
        bytes(header[64:576]).split(b"\0", 1)[0]
        + bytes(header[608:1632]).split(b"\0", 1)[0]
    )


def build(reference_path: Path, output_path: Path) -> tuple[bytes, str]:
    packer = packer_module()
    reference = reference_path.read_bytes()
    reference_header, reference_parts, _page = packer.components(reference)
    output = packer.pack(
        reference,
        reference_parts["kernel"],
        append_cmdline=DEBUG_ARGUMENT,
    )
    output_header, output_parts, _page = packer.components(output)

    for name in ("kernel", "ramdisk", "second", "recovery_dtbo", "dtb"):
        if output_parts[name] != reference_parts[name]:
            raise ValueError(f"output changed the source {name} payload")
    if output_header[:608] != reference_header[:608]:
        raise ValueError("output changed boot-header fields before extra_cmdline")
    if output_header[1632:] != reference_header[1632:]:
        raise ValueError("output changed boot-header fields after extra_cmdline")

    original_cmdline = command_line(reference_header)
    expected_separator = (
        b" " if original_cmdline and not original_cmdline.endswith((b" ", b"\t"))
        else b""
    )
    expected_cmdline = original_cmdline + expected_separator + DEBUG_ARGUMENT.encode("ascii")
    if command_line(output_header) != expected_cmdline:
        raise ValueError("output command line does not contain the separated debug argument")

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_bytes(output)
    return output, hashlib.sha256(output).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, default=DEFAULT_REFERENCE)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()
    output, digest = build(args.reference, args.output)
    print(f"built {args.output} ({len(output)} bytes; sha256={digest})")
    print("all payloads preserved; temporary fastboot boot only; no partition flash")


if __name__ == "__main__":
    main()
