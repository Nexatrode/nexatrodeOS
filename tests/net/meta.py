# Copyright 2026, Personal OS contributors
# SPDX-License-Identifier: BSD-2-Clause
"""Flat single-client VirtIO net fragment (no serial, no benchmark nesting)."""
import argparse
import os
import sys
from typing import Optional

from sdfgen import SystemDescription, Sddf, DeviceTree

sys.path.append(
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../.cache/sddf-personal-os/tools/meta")
)
# Prefer the overlay SDDF meta path when the build passes --sddf.
def _board_path(sddf_root: str) -> str:
    return os.path.join(sddf_root, "tools", "meta")


ProtectionDomain = SystemDescription.ProtectionDomain


def generate(sdf_file: str, output_dir: str, dtb: Optional[DeviceTree], board, sdf, sddf):
    ethernet_node = dtb.node(board.ethernet)
    assert ethernet_node is not None
    timer_node = dtb.node(board.timer)
    assert timer_node is not None

    timer_driver = ProtectionDomain("timer_driver", "timer_driver.elf", priority=102)
    timer_system = Sddf.Timer(sdf, timer_node, timer_driver)

    ethernet_driver = ProtectionDomain(
        "ethernet_driver",
        "eth_driver.elf",
        priority=101,
        budget=100,
        period=400,
    )
    net_virt_tx = ProtectionDomain(
        "net_virt_tx", "network_virt_tx.elf", priority=100, budget=20000
    )
    net_virt_rx = ProtectionDomain(
        "net_virt_rx", "network_virt_rx.elf", priority=99
    )
    net_system = Sddf.Net(sdf, ethernet_node, ethernet_driver, net_virt_tx, net_virt_rx)

    dhcp_client = ProtectionDomain(
        "dhcp_client", "dhcp_client.elf", priority=97, budget=20000
    )
    net_copier = ProtectionDomain(
        "net_copier", "network_copy.elf", priority=98, budget=20000
    )

    timer_system.add_client(dhcp_client)
    net_system.add_client_with_copier(dhcp_client, net_copier)
    client_lwip = Sddf.Lwip(sdf, net_system, dhcp_client)

    for pd in (
        timer_driver,
        ethernet_driver,
        net_virt_tx,
        net_virt_rx,
        dhcp_client,
        net_copier,
    ):
        sdf.add_pd(pd)

    assert net_system.connect()
    assert net_system.serialise_config(output_dir)
    assert timer_system.connect()
    assert timer_system.serialise_config(output_dir)
    assert client_lwip.connect()
    assert client_lwip.serialise_config(output_dir)

    with open(f"{output_dir}/{sdf_file}", "w+") as handle:
        handle.write(sdf.render())


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--dtb", required=True)
    parser.add_argument("--sddf", required=True)
    parser.add_argument("--board", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--sdf", required=True)
    args = parser.parse_args()

    sys.path.insert(0, _board_path(args.sddf))
    from board import BOARDS

    board = next(filter(lambda entry: entry.name == args.board, BOARDS))
    if board.name != "qemu_virt_aarch64":
        raise SystemExit("tests/net only targets qemu_virt_aarch64")

    sdf = SystemDescription(board.arch, board.paddr_top)
    sddf = Sddf(args.sddf)
    with open(args.dtb, "rb") as handle:
        dtb = DeviceTree(handle.read())
    generate(args.sdf, args.output, dtb, board, sdf, sddf)
