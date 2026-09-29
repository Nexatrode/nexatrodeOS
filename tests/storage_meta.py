#!/usr/bin/env python3
"""Generate a minimal sDDF system with the Rust vault as its block client."""
import argparse
from pathlib import Path
import shutil
import sys

from sdfgen import DeviceTree, Sddf, SystemDescription


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--sddf", required=True)
    parser.add_argument("--board", required=True)
    parser.add_argument("--dtb", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--sdf", required=True)
    parser.add_argument("--partition", type=int, default=0)
    parser.add_argument("--need_timer", action="store_true")
    parser.add_argument("--nvme", action="store_true")
    args = parser.parse_args()

    if args.board != "qemu_virt_aarch64" or args.need_timer or args.nvme:
        parser.error("this storage test targets QEMU virt with the sDDF virtio block driver")

    sys.path.insert(0, str(Path(args.sddf) / "tools/meta"))
    from board import BOARDS

    board = next(item for item in BOARDS if item.name == args.board)
    sdf = SystemDescription(board.arch, board.paddr_top)
    dtb = DeviceTree(Path(args.dtb).read_bytes())
    device = dtb.node(board.blk)
    uart = dtb.node(board.serial)
    if device is None:
        raise RuntimeError(f"could not find block device {board.blk} in the device tree")
    if uart is None:
        raise RuntimeError(f"could not find UART {board.serial} in the device tree")

    driver = SystemDescription.ProtectionDomain(
        "blk_driver", "blk_driver.elf", priority=200, stack_size=0x2000
    )
    virtualiser = SystemDescription.ProtectionDomain(
        "blk_virt", "blk_virt.elf", priority=199, stack_size=0x2000
    )
    vault = SystemDescription.ProtectionDomain(
        "vault", "rust_vault_disk.elf", priority=100, passive=True,
        stack_size=0x10000,
    )
    tester = SystemDescription.ProtectionDomain(
        "storage_test", "rust_vault_test_client.elf", priority=99,
        budget=3_000, period=10_000,
    )
    serial_driver = SystemDescription.ProtectionDomain(
        "serial_driver", "serial_driver.elf", priority=200
    )
    serial_virtualiser = SystemDescription.ProtectionDomain(
        "serial_virt_tx", "serial_virt_tx.elf", priority=199
    )
    entropy = SystemDescription.ProtectionDomain(
        "entropy_rng", "rust_entropy_rng.elf", priority=102, passive=True,
        stack_size=0x4000,
    )

    block = Sddf(args.sddf)
    blk_system = block.Blk(sdf, device, driver, virtualiser)
    blk_system.add_client(
        vault,
        partition=args.partition,
        queue_capacity=32,
        data_size=8 * 4096,
    )
    serial_system = block.Serial(
        sdf, uart, serial_driver, serial_virtualiser, enable_color=False
    )
    serial_system.add_client(tester)

    private = SystemDescription.MemoryRegion(sdf, "vault_private", 0x1000)
    sdf.add_mr(private)
    vault.add_map(
        SystemDescription.Map(
            private, 0x42000000, "rw", setvar_vaddr="vault_private_start"
        )
    )
    # The disk-test image reuses the production vault binary. Its Notes page
    # is shared with the test client so blob commits can be interrupted at the
    # same flush boundaries as scalar commits.
    for name, vaddr in (("notes_transfer", 0x45000000),
                        ("contacts_transfer", 0x46000000)):
        transfer = SystemDescription.MemoryRegion(sdf, name, 0x1000)
        sdf.add_mr(transfer)
        vault.add_map(
            SystemDescription.Map(
                transfer, vaddr, "rw", cached=False, setvar_vaddr=name
            )
        )
        if name == "notes_transfer":
            tester.add_map(
                SystemDescription.Map(
                    transfer, 0x22002000, "rw", cached=False,
                    setvar_vaddr="blob_transfer",
                )
            )
    rng_mmio = SystemDescription.MemoryRegion(
        sdf, "rng_mmio", 0x1000, paddr=0x0a001000
    )
    sdf.add_mr(rng_mmio)
    entropy.add_map(
        SystemDescription.Map(
            rng_mmio, 0x52000000, "rw", cached=False, setvar_vaddr="rng_regs"
        )
    )
    rng_dma = SystemDescription.MemoryRegion(sdf, "rng_dma", 0x2000, paddr=0x5fddc000)
    sdf.add_mr(rng_dma)
    entropy.add_map(
        SystemDescription.Map(
            rng_dma, 0x53000000, "rw", cached=False, setvar_vaddr="rng_dma"
        )
    )
    for pd in (driver, virtualiser, vault, tester,
               serial_driver, serial_virtualiser, entropy):
        sdf.add_pd(pd)

    if not blk_system.connect():
        raise RuntimeError("sDDF block system could not connect")
    output = Path(args.output)
    if not blk_system.serialise_config(str(output)):
        raise RuntimeError("sDDF block configuration could not be generated")
    if not serial_system.connect():
        raise RuntimeError("sDDF serial system could not connect")
    if not serial_system.serialise_config(str(output)):
        raise RuntimeError("sDDF serial configuration could not be generated")

    # The example Makefile expects this conventional filename when it patches
    # the unused C client image. The Rust vault receives the same generated bytes
    # separately in the host build script.
    shutil.copyfile(output / "blk_client_vault.data", output / "blk_client_client.data")
    shutil.copyfile(output / "serial_client_storage_test.data",
                    output / "serial_client_client.data")

    owner = SystemDescription.Channel(
        tester,
        vault,
        a_id=1,
        b_id=1,
        pp_a=True,
        notify_a=False,
        notify_b=False,
    )
    sdf.add_channel(owner)
    notes = SystemDescription.Channel(
        tester,
        vault,
        a_id=3,
        b_id=3,
        pp_a=True,
        notify_a=False,
        notify_b=False,
    )
    sdf.add_channel(notes)
    entropy_channel = SystemDescription.Channel(
        tester,
        entropy,
        a_id=2,
        b_id=0,
        pp_a=True,
        notify_a=False,
        notify_b=False,
    )
    sdf.add_channel(entropy_channel)

    (output / args.sdf).write_text(sdf.render())


if __name__ == "__main__":
    main()
