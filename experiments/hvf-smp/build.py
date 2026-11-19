"""Build a disposable, pinned Microkit SMP SDK and an affinity probe for Apple HVF."""
import argparse
import dataclasses
import os
from pathlib import Path
import shutil
import subprocess
import sys
from xml.etree import ElementTree as ET

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
BOARD = "qemu_virt_aarch64_el1"
CONFIG = "smp-debug"
VERSION = "2.3.1"
MICROKIT = "ec86afdcd662b5976d11d4994acf1b11a2979882"
SEL4 = "6e7c3b733d296cfd88d5fbf635c96e447a882374"


def checkout(directory, cache, project, revision):
    # Fetch committed objects only, including when the local source tree has edits.
    subprocess.run(["git", "init", "--quiet", directory], check=True)
    source = str(cache) if cache.is_dir() else f"https://github.com/seL4/{project}.git"
    subprocess.run(["git", "-C", directory, "fetch", "--quiet", "--depth=1", source, revision], check=True)
    subprocess.run(["git", "-C", directory, "checkout", "--quiet", "--detach", "FETCH_HEAD"], check=True)


def replace_exact(path, old, new, count):
    contents = path.read_text()
    if contents.count(old) != count:
        raise RuntimeError(f"Unexpected source in {path}")
    path.write_text(contents.replace(old, new))


def build_sdk(out, args):
    microkit = out / "microkit"
    sel4 = out / "sel4"
    checkout(microkit, ROOT / ".cache/microkit-src-2.3.1", "microkit", MICROKIT)
    checkout(sel4, ROOT / ".cache/sel4-src-microkit231", "seL4", SEL4)
    if not args.original_gicr:
        subprocess.run(["git", "-C", sel4, "apply", HERE / "gicr-typer.patch"], check=True)
    # The HVF Arm generic timer runs at 24 MHz on the tested Apple Silicon Mac.
    replace_exact(sel4 / "src/plat/qemu-arm-virt/config.cmake",
                  "TIMER_FREQUENCY 62500000", "TIMER_FREQUENCY 24000000", 1)
    if args.conduit == "hvc":
        replace_exact(microkit / "loader/src/aarch64/smc.h", '"smc #0', '"hvc #0', 2)

    os.chdir(microkit)
    sys.path.insert(0, str(microkit))
    import build_sdk as upstream

    board = next(board for board in upstream.SUPPORTED_BOARDS if board.name == BOARD)
    options = dict(board.kernel_options, QEMU_GIC_VERSION="3", CMAKE_EXE_LINKER_FLAGS="-fuse-ld=lld")
    upstream.SUPPORTED_BOARDS = (dataclasses.replace(board, smp_cores=args.cores, kernel_options=options),)
    sys.argv = ["build_sdk.py", "--sel4", str(sel4),
                "--boards", BOARD, "--configs", CONFIG, "--llvm", "--skip-tool",
                "--skip-docs", "--skip-tar", "--skip-initialiser"]
    upstream.main()
    sdk = microkit / f"release/microkit-sdk-{VERSION}"
    board_config = Path("board") / BOARD / CONFIG
    initialiser = board_config / "elf/initialiser.elf"
    shutil.copy2(args.release_sdk / initialiser, sdk / initialiser)
    shutil.copy2(args.release_sdk / "bin/microkit", sdk / "bin/microkit")
    (out / "sdk").symlink_to(sdk, target_is_directory=True)
    return sdk, sdk / board_config


def compose(sdk, board_config, out, cores):
    system = ET.Element("system")
    ET.SubElement(system, "memory_region", name="progress", size="0x1000")
    for cpu in range(cores):
        name = f"cpu{cpu}"
        obj = out / f"{name}.o"
        subprocess.run(["clang", "--target=aarch64-none-elf", "-mcpu=cortex-a53",
                        "-mgeneral-regs-only", "-std=gnu11", "-O2", "-ffreestanding",
                        "-nostdlib", "-Wall", "-Werror", "-I", board_config / "include",
                        f"-DCPU={cpu}", f"-DNCPU={cores}", "-c", HERE / "probe.c", "-o", obj], check=True)
        subprocess.run(["ld.lld", "-L", board_config / "lib", obj, "-lmicrokit",
                        "-T", "microkit.ld", "-o", out / f"{name}.elf"], check=True)
        pd = ET.SubElement(system, "protection_domain", name=name, cpu=str(cpu),
                           priority="100", budget="10000", period="10000")
        ET.SubElement(pd, "program_image", path=f"{name}.elf")
        ET.SubElement(pd, "map", mr="progress", vaddr="0x40000000", perms="rw",
                      cached="true", setvar_vaddr="shared_addr")
    ET.indent(system)
    sdf = out / "probe.system"
    ET.ElementTree(system).write(sdf, encoding="utf-8")
    subprocess.run([sdk / "bin/microkit", sdf, "--search-path", out, "--board", BOARD,
                    "--config", CONFIG, "-o", out / "probe.img", "-r", out / "probe.report"], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cores", type=int, choices=(2, 4))
    parser.add_argument("--conduit", choices=("hvc", "smc"), default="hvc")
    parser.add_argument("--original-gicr", action="store_true",
                        help="build the original redistributor read for an A/B control")
    parser.add_argument("--release-sdk", type=Path, default=ROOT / f".cache/microkit-sdk-{VERSION}")
    parser.add_argument("--output", type=Path, help="new build directory")
    args = parser.parse_args()
    args.release_sdk = args.release_sdk.resolve()
    if not (args.release_sdk / "board" / BOARD / CONFIG / "elf/initialiser.elf").is_file():
        parser.error(f"Microkit {VERSION} SDK missing at {args.release_sdk}. Install it with ./dev setup.")
    variant = "original" if args.original_gicr else "fixed"
    out = (args.output or ROOT / "build/hvf-smp" / f"{args.cores}-{args.conduit}-{variant}").resolve()
    if out.exists():
        parser.error(f"Output already exists at {out}. Choose a new --output directory.")
    out.mkdir(parents=True)
    sdk, board_config = build_sdk(out, args)
    compose(sdk, board_config, out, args.cores)
    print(f"SDK {out / 'sdk'}\nProbe {out / 'probe.img'}")


if __name__ == "__main__":
    main()
