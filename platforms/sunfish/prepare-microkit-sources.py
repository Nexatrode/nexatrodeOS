#!/usr/bin/env python3
"""Apply the Pixel 4a board glue to separate, disposable upstream worktrees.

The source checkout and live boot image remain outside the Git-tracked port.
This script is idempotent and refuses to silently rewrite unexpected upstream
versions. It only prepares a build; it never contacts or flashes a phone.
"""

from pathlib import Path
import re
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[2]
PORT = ROOT / "platforms/sunfish"
CACHE = ROOT / ".cache"
SEL4 = CACHE / "sel4-src-16.0-sunfish"
MICROKIT = CACHE / "microkit-src-2.3.1-sunfish"
LIVE_DTB = CACHE / "pixel-sunfish/live-runtime.dtb"
LIVE_DTS = CACHE / "pixel-sunfish/live-runtime.dts"


def insert_once(path: Path, anchor: str, addition: str) -> None:
    source = path.read_text()
    if addition in source:
        return
    if source.count(anchor) != 1:
        raise RuntimeError(f"unexpected upstream layout: {path}: {anchor!r}")
    path.write_text(source.replace(anchor, addition + anchor, 1))


def main() -> None:
    for path in (SEL4, MICROKIT, LIVE_DTB):
        if not path.exists():
            raise RuntimeError(f"missing {path}")

    # Use the final FDT passed to the running Android kernel, after the active
    # device-specific DTBO has been merged. The boot_a image's embedded base
    # DTB is not sufficient for Microkit resource accounting. Capture this
    # file read-only from /sys/firmware/fdt on the target Lineage boot.
    # Always regenerate the text form: the runtime FDT may have been recaptured
    # after a Lineage or DTBO update, and a cached DTS must not silently lag it.
    with LIVE_DTS.open("wb") as output:
        subprocess.run(["dtc", "-I", "dtb", "-O", "dts", "-f", str(LIVE_DTB)],
                       stdout=output, check=True)

    platform = SEL4 / "src/plat/sunfish"
    platform.mkdir(exist_ok=True)
    shutil.copyfile(PORT / "sel4-config.cmake", platform / "config.cmake")
    shutil.copyfile(PORT / "sel4-geni-uart.c", SEL4 / "src/drivers/serial/geni-uart.c")
    constants = SEL4 / "libsel4/sel4_plat_include/sunfish/sel4/plat/api/constants.h"
    constants.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(PORT / "sel4-constants.h", constants)

    insert_once(
        SEL4 / "src/drivers/serial/config.cmake",
        'register_driver(compatibility_strings "qcom,msm-uartdm"',
        'register_driver(compatibility_strings "qcom,msm-geni-console;qcom,geni-debug-uart" '
        'PREFIX src/drivers/serial CFILES "geni-uart.c")\n',
    )
    insert_once(
        SEL4 / "tools/hardware.yml",
        "      - qcom,msm-uartdm\n",
        "      - qcom,msm-geni-console\n      - qcom,geni-debug-uart\n",
    )

    board = f'''    BoardInfo(
        name="sunfish",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a55",
        loader_link_address=0x81000000,
        kernel_options={{
            "KernelPlatform": "sunfish",
            "KernelArmHypervisorSupport": False,
            "KernelCustomDTS": Path("{LIVE_DTS}"),
            "KernelCustomDTSOverlay": Path("{PORT / 'sel4-overlay.dts'}"),
        }},
    ),
'''
    board_path = MICROKIT / "build_sdk.py"
    board_source = board_path.read_text()
    board_pattern = re.compile(
        r'    BoardInfo\(\n        name="sunfish",.*?    \),\n', re.DOTALL
    )
    # This generated entry may already exist from a previous preparation run
    # with an older DTB path. Remove all such entries before inserting exactly
    # one current definition, so repeated runs cannot leave a stale duplicate.
    board_source = board_pattern.sub("", board_source)
    anchor = "SUPPORTED_BOARDS = (\n"
    if board_source.count(anchor) != 1:
        raise RuntimeError("unexpected upstream board list")
    board_path.write_text(board_source.replace(anchor, anchor + board, 1))

    loader_uart = (PORT / "loader-geni-uart.inc").read_text()
    insert_once(MICROKIT / "loader/src/uart.c", "#elif defined(CONFIG_PLAT_QEMU_ARM_VIRT)", loader_uart)
    print(f"Prepared {SEL4} and {MICROKIT} from {LIVE_DTB}")


if __name__ == "__main__":
    main()
